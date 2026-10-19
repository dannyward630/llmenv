//! Removal of explicit JSON nulls after a native overlay (#1264).

/// Recursively remove null-valued keys from every JSON object in `value`.
///
/// #1264: a native `null` on a key the renderer already emitted lands as an
/// explicit JSON `null` rather than deleting the key — `merge_json`'s
/// shared-key overwrite arm deliberately does not null-strip (an explicit
/// null is intentional data, not an `Option::None` artifact), so the caller
/// must strip at the write boundary instead. Every adapter write path that
/// overlays a `native*` catch-all fragment onto already-rendered output calls
/// this as its last step, after the final overlay, so it catches every layer.
///
/// Also makes objects that differ only by null vs absent key compare equal
/// under [`PartialEq`], which `merge_json`'s array dedup relies on.
pub(crate) fn strip_json_nulls(value: &mut serde_json::Value) {
    strip_json_nulls_depth(value, 0);
}

/// Depth-limited implementation of [`strip_json_nulls`].
///
/// The depth guard prevents stack overflow on pathological JSON nesting
/// (config depth is normally <10 levels). The serde_json parser has its own
/// recursion limit, but that guards _parsing_ — the value tree can be
/// arbitrarily nested after deserialization.
///
/// # Fail-open, deliberately (#1274)
///
/// Bailing past depth 64 leaves the remaining subtree's nulls unstripped,
/// which can now reach persistent files this function's callers write to
/// directly (`.claude.json`, `settings.json`, `crush.json`, `opencode.json`)
/// — not just rebuildable cache output. Fail-closed (propagating an error
/// instead of bailing) was considered and rejected — not because every
/// caller would need reworking (four of this function's six call sites
/// already return `anyhow::Result` and use `?`: `merge_mcp_into_claude_json`,
/// `generate_settings_json`, opencode's and crush's `materialize`), but
/// because the other two (`dedup_hooks_doc`'s call inside a `.map()`/
/// `.or_else()` merge chain, and `normalized_hook` called from a
/// `Vec::retain` predicate, both in `claude_code.rs`) can't return early with
/// `?` without restructuring those combinators. Making four call sites
/// fail-closed while the other two stay fail-open would be an inconsistent
/// half-fix for an input that must already survive a full `serde_yaml`/
/// `serde_json` parse to reach 64 levels of nesting, and the null surviving
/// past the guard is at worst equivalent to the key being absent — the
/// deny-never-weakened invariant is enforced by
/// `reject_modeled_keys_in_catch_all`, not by null-stripping. Realistic worst
/// case stays what #1274 found it to be: a cosmetically wrong config, never
/// a panic or crash.
fn strip_json_nulls_depth(value: &mut serde_json::Value, depth: usize) {
    if depth > 64 {
        tracing::error!(
            "strip_json_nulls: exceeded max depth (64) — bailing on the remaining subtree, \
             which may leave stray null values in a persistent config file"
        );
        return;
    }
    match value {
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                strip_json_nulls_depth(item, depth + 1);
            }
        }
        serde_json::Value::Object(map) => {
            map.retain(|_, v| !v.is_null());
            for v in map.values_mut() {
                strip_json_nulls_depth(v, depth + 1);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::Value;

    use super::*;

    fn arb_value() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            "[a-z]{0,3}".prop_map(Value::String),
        ];
        leaf.prop_recursive(5, 40, 4, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
                proptest::collection::btree_map("[a-c]", inner, 0..4)
                    .prop_map(|m| Value::Object(m.into_iter().collect())),
            ]
        })
    }

    /// No object key holds a null after the strip.
    fn no_null_key(value: &Value) -> bool {
        match value {
            Value::Object(map) => map.values().all(|v| !v.is_null() && no_null_key(v)),
            Value::Array(items) => items.iter().all(no_null_key),
            _ => true,
        }
    }

    /// The shape of `value` with each null-valued key dropped, built by the test as the oracle.
    fn oracle(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| (k.clone(), oracle(v)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(oracle).collect()),
            other => other.clone(),
        }
    }

    proptest! {
        /// Within the depth limit, every null-valued key goes, a null array item stays, the rest
        /// is unchanged, and a second strip changes nothing.
        #[test]
        fn the_strip_drops_null_keys_only(value in arb_value()) {
            let mut once = value.clone();
            strip_json_nulls(&mut once);
            prop_assert!(no_null_key(&once));
            prop_assert_eq!(&once, &oracle(&value));
            let mut twice = once.clone();
            strip_json_nulls(&mut twice);
            prop_assert_eq!(twice, once);
        }
    }
}
