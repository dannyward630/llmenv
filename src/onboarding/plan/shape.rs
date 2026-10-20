//! The shape rules of a plan. They keep every entry that can run code or grant a tool inside
//! the lists that the gate asks about (Q4 of docs/design/issue-2773-onboarding.md).

use std::collections::BTreeSet;

use anyhow::{bail, ensure};
use serde_yaml::Value;

use super::{Plan, shown};

/// The capability keys that a plan bundle may set. Every entry of these lists, except a
/// permission `ask` or `deny`, gets a question. Other keys, such as `env` or `skills`, can run
/// code or grant tools in ways the gate cannot show, so they are added by hand after setup.
const PLAN_CAPABILITY_KEYS: [&str; 5] = ["permissions", "hooks", "plugins", "mcp", "lsp"];

/// The capability keys that a shipped bundle may set: the plan keys, and `env`, whose variables
/// the gate asks about one by one. The shipped files are copies of another repository, so a later
/// copy that adds a key the gate cannot show fails the tests instead of reaching a user.
const SHIPPED_CAPABILITY_KEYS: [&str; 6] = ["permissions", "hooks", "plugins", "mcp", "lsp", "env"];

/// The keys of `permissions` that a plan may set. `default_mode` and `preset` change what the
/// agent may do without a list entry the gate could show, so a plan cannot set them.
const PLAN_PERMISSION_KEYS: [&str; 3] = ["allow", "ask", "deny"];

/// Fail on a YAML tag anywhere in `value`. A tag on a key hides the key from an exact lookup,
/// while the loader that reads the written file can still see it as the plain key.
pub(super) fn check_no_tags(value: &Value, place: &str) -> anyhow::Result<()> {
    match value {
        Value::Tagged(tagged) => bail!(
            "{place} holds the YAML tag {}. Fix: remove the tag; a plan does not use tags",
            shown(&tagged.tag.to_string())
        ),
        Value::Sequence(items) => items.iter().try_for_each(|v| check_no_tags(v, place)),
        Value::Mapping(map) => map.iter().try_for_each(|(k, v)| {
            check_no_tags(k, place)?;
            check_no_tags(v, place)
        }),
        _ => Ok(()),
    }
}

/// The keys of `map` as strings. A key that is not a string fails, and names `place`.
fn string_keys<'a>(map: &'a serde_yaml::Mapping, place: &str) -> anyhow::Result<Vec<&'a str>> {
    map.keys()
        .map(|k| {
            k.as_str().ok_or_else(|| {
                anyhow::anyhow!(
                    "{place} has a key that is not a string: {}",
                    shown(&format!("{k:?}"))
                )
            })
        })
        .collect()
}

impl Plan {
    pub(super) fn check_shape(&self) -> anyhow::Result<()> {
        let mut seen = BTreeSet::new();
        for bundle in &self.bundles {
            let name = &bundle.name;
            ensure!(
                llmenv_paths::is_valid_short_name(name),
                "bundle name {} is not a plain name (letters, digits, `.`, `_`, `-`)",
                shown(name)
            );
            ensure!(
                seen.insert(name.clone()),
                "bundle {name:?} is in the plan twice"
            );
            check_capabilities(name, &bundle.capabilities, &PLAN_CAPABILITY_KEYS)?;
        }
        string_keys(&self.host, "the plan's `host`")?;
        for (kind, entries) in self.scope.kinds() {
            for entry in entries {
                ensure!(
                    entry
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| !id.is_empty()),
                    "a {kind} scope entry in the plan has no `id`. Fix: give each scope an id"
                );
            }
        }
        Ok(())
    }
}

/// The same rules for the capabilities of the shipped bundle `name`, with `env` allowed as a
/// mapping. Every entry then lands in a list or a mapping that the gate asks about.
pub(super) fn check_shipped(name: &str, caps: &serde_yaml::Mapping) -> anyhow::Result<()> {
    check_no_tags(&Value::Mapping(caps.clone()), "the shipped bundle")?;
    check_capabilities(name, caps, &SHIPPED_CAPABILITY_KEYS)?;
    if let Some(env) = caps.get("env") {
        ensure!(env.is_mapping(), "bundle {name:?}: `env` must be a mapping");
    }
    Ok(())
}

fn check_capabilities(
    name: &str,
    caps: &serde_yaml::Mapping,
    allowed: &[&str],
) -> anyhow::Result<()> {
    let place = format!("bundle {name:?} capabilities");
    for key in string_keys(caps, &place)? {
        ensure!(
            allowed.contains(&key),
            "bundle {name:?}: an onboarding plan cannot set `{}`. Fix: leave it out, and add it to \
             bundle.yaml by hand after setup. Allowed keys: {}",
            shown(key),
            allowed.join(", ")
        );
    }
    // The gate counts the entries of these lists. A list in another shape would count as none,
    // so it fails here instead.
    for (key, sub, _) in super::gate::GATED_LISTS {
        let value = caps.get(key).map(|v| sub.map_or(Some(v), |s| v.get(s)));
        if let Some(Some(value)) = value {
            ensure!(
                value.is_sequence(),
                "bundle {name:?}: `{key}{}` must be a list",
                sub.map(|s| format!(".{s}")).unwrap_or_default()
            );
        }
    }
    let Some(permissions) = caps.get("permissions") else {
        return Ok(());
    };
    let Some(permissions) = permissions.as_mapping() else {
        bail!("bundle {name:?}: `permissions` must be a mapping of allow, ask, and deny lists");
    };
    for key in string_keys(permissions, &place)? {
        ensure!(
            PLAN_PERMISSION_KEYS.contains(&key),
            "bundle {name:?}: an onboarding plan cannot set `permissions.{}`. Fix: leave it out, \
             and add it to bundle.yaml by hand after setup. Allowed keys: {}",
            shown(key),
            PLAN_PERMISSION_KEYS.join(", ")
        );
    }
    Ok(())
}
