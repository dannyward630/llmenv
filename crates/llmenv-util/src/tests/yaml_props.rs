use super::{dedup, merge_yaml};
use proptest::prelude::*;
use serde_yaml::Value;

/// A small recursive YAML-value generator mirroring `arb_json`'s shape
/// (scalars, then sequences/mappings of them) — local to this crate's
/// own tests, not shared via `testkit` (see `testkit`'s module doc for
/// why an `arb_yaml_value` isn't unified across crates).
fn arb_yaml() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i32>().prop_map(Value::from),
        "[a-z]{0,4}".prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Sequence),
            prop::collection::vec(("[a-z]{1,4}", inner), 0..4).prop_map(|kvs| {
                Value::Mapping(
                    kvs.into_iter()
                        .map(|(k, v)| (Value::String(k), v))
                        .collect(),
                )
            }),
        ]
    })
}

proptest! {
    // merge_yaml never panics on arbitrary input pairs.
    #[test]
    fn merge_yaml_total(mut dst in arb_yaml(), src in arb_yaml()) {
        merge_yaml(&mut dst, src);
    }

    // Disjoint mapping keys survive the merge; shared keys take src's
    // value when both are scalars (src wins on scalar collision).
    #[test]
    fn merge_yaml_src_scalar_wins_on_shared_key(
        key in "[a-z]{1,4}",
        a in any::<i32>(),
        b in any::<i32>(),
    ) {
        let mut dst = Value::Mapping([(Value::String(key.clone()), Value::from(a))].into_iter().collect());
        merge_yaml(&mut dst, Value::Mapping([(Value::String(key.clone()), Value::from(b))].into_iter().collect()));
        prop_assert_eq!(dst.get(&key).cloned(), Some(Value::from(b)));
    }

    // Merging a value into itself is idempotent once sequences are
    // dedup-stable: re-merging the result changes nothing.
    #[test]
    fn merge_yaml_idempotent(v in arb_yaml()) {
        let mut once = v.clone();
        merge_yaml(&mut once, v.clone());
        let mut twice = once.clone();
        merge_yaml(&mut twice, once.clone());
        prop_assert_eq!(once, twice);
    }

    // Sequence merge output carries no duplicates (concat + dedup).
    #[test]
    fn merge_yaml_sequences_dedup(
        a in prop::collection::vec(0i32..5, 0..6),
        b in prop::collection::vec(0i32..5, 0..6),
    ) {
        let mut dst = Value::Sequence(a.iter().map(|n| Value::from(*n)).collect());
        merge_yaml(&mut dst, Value::Sequence(b.iter().map(|n| Value::from(*n)).collect()));
        let seq = dst.as_sequence().unwrap();
        let mut seen = seq.clone();
        dedup(&mut seen);
        prop_assert_eq!(seq.len(), seen.len(), "no duplicates in merged sequence");
    }

    // Stronger idempotence: merging ANY src into ANY dst is idempotent —
    // re-applying the same src to the merged result is a no-op.
    #[test]
    fn merge_yaml_idempotent_for_arbitrary_pairs(
        dst in arb_yaml(),
        src in arb_yaml(),
    ) {
        let mut once = dst;
        merge_yaml(&mut once, src.clone());
        let mut twice = once.clone();
        merge_yaml(&mut twice, src);
        prop_assert_eq!(once, twice);
    }

    // Normalization is preserved: if `dst` is already dedup-free, then
    // merging arbitrary `src` keeps the output dedup-free at every depth.
    #[test]
    fn merge_yaml_preserves_normalization(
        dst in arb_yaml(),
        src in arb_yaml(),
    ) {
        let mut normalized = Value::Null;
        merge_yaml(&mut normalized, dst);
        prop_assume!(all_sequences_deduped(&normalized));

        merge_yaml(&mut normalized, src);
        prop_assert!(
            all_sequences_deduped(&normalized),
            "merge introduced a non-deduped sequence: {normalized:?}"
        );
    }
}

// True iff every sequence nested anywhere in `v` contains no duplicates.
fn all_sequences_deduped(v: &Value) -> bool {
    match v {
        Value::Sequence(items) => {
            let mut seen = items.clone();
            dedup(&mut seen);
            seen.len() == items.len() && items.iter().all(all_sequences_deduped)
        }
        Value::Mapping(map) => map.values().all(all_sequences_deduped),
        _ => true,
    }
}
