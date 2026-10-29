//! Checks on the marketplaces in the config: a declared name that differs from the config key, and
//! a marketplace that no plugin collection uses.

use std::collections::HashSet;
use std::path::Path;

use crate::config::{Config, split_plugin_ref};
use crate::plugins::cache;
use crate::util::display_safe;

/// Warn for each marketplace clone whose manifest names it differently from the config key.
/// Claude Code registers a clone under the manifest name, so its plugins never load (#3048).
pub(super) fn report_declared_names(config: &Config, cache_dir: &Path, warn: &str) {
    for problem in declared_name_problems(config, cache_dir) {
        eprintln!("{warn} {problem}");
    }
}

/// One line per marketplace whose declared name differs from the config key, or whose manifest
/// cannot be read. A clone that is not synced yet has no manifest, so it is not a problem.
fn declared_name_problems(config: &Config, cache_dir: &Path) -> Vec<String> {
    config
        .marketplace
        .iter()
        .filter_map(|m| {
            let name = display_safe(&m.name);
            match cache::read_marketplace_name(&cache::marketplace_path(cache_dir, &m.name)) {
                Ok(Some(declared)) if declared != m.name => {
                    let declared = display_safe(&declared);
                    Some(format!(
                        "marketplace '{name}' declares name '{declared}' in its manifest, so its \
                         plugins do not load. Fix: rename it to '{declared}' in config.yaml, and \
                         use '{declared}:<plugin>'."
                    ))
                }
                Ok(_) => None,
                Err(e) => Some(format!(
                    "marketplace '{name}': cannot read its manifest name: {e:#}"
                )),
            }
        })
        .collect()
}

/// The marketplace of each plugin reference in every plugin collection. A reference with no
/// marketplace part is logged and keeps nothing, because sync refuses such a config.
pub(super) fn referenced_marketplaces(config: &Config) -> HashSet<&str> {
    config
        .plugin_collection
        .iter()
        .flat_map(|c| c.plugins.iter())
        .filter_map(|p| match split_plugin_ref(p) {
            Some((marketplace, _)) => Some(marketplace),
            None => {
                tracing::warn!(
                    "plugin reference '{}' has no marketplace part, so it keeps no clone. Fix: \
                     write it as <marketplace>:<plugin>.",
                    display_safe(p)
                );
                None
            }
        })
        .collect()
}

/// Returns marketplace names defined in `config` that no plugin collection references.
pub(super) fn unused_marketplaces(config: &Config) -> Vec<&str> {
    let referenced = referenced_marketplaces(config);
    config
        .marketplace
        .iter()
        .filter(|m| !referenced.contains(m.name.as_str()))
        .map(|m| m.name.as_str())
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: a failed setup step should abort the test"
)]
mod tests {
    use std::fs;

    use super::*;
    use crate::config::Marketplace;

    fn config_with_marketplace(name: &str) -> Config {
        Config {
            marketplace: vec![Marketplace {
                name: name.into(),
                source: "https://example.com".into(),
            }],
            ..Config::default()
        }
    }

    fn write_manifest(cache_dir: &Path, clone: &str, body: &str) {
        let dir = crate::plugins::cache::marketplace_path(cache_dir, clone).join(".claude-plugin");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("marketplace.json"), body).unwrap();
    }

    proptest::proptest! {
        #[test]
        fn a_declared_name_is_a_problem_exactly_when_it_differs_from_the_key(
            key in "[a-z][a-z0-9-]{0,11}",
            declared in "[a-z][a-z0-9-]{0,11}",
        ) {
            let cache = tempfile::tempdir().unwrap();
            write_manifest(cache.path(), &key, &format!(r#"{{"name": "{declared}"}}"#));
            let problems = declared_name_problems(&config_with_marketplace(&key), cache.path());
            proptest::prop_assert_eq!(problems.is_empty(), key == declared);
        }
    }

    #[test]
    fn a_declared_name_that_differs_from_the_config_key_is_a_problem() {
        let cache = tempfile::tempdir().unwrap();
        write_manifest(cache.path(), "old-key", r#"{"name": "new-name"}"#);
        let config = config_with_marketplace("old-key");

        let problems = declared_name_problems(&config, cache.path());

        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("declares name 'new-name'"),
            "{problems:?}"
        );
        assert!(
            problems[0].contains("Fix: rename it to 'new-name'"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_matching_declared_name_is_not_a_problem() {
        let cache = tempfile::tempdir().unwrap();
        write_manifest(cache.path(), "same", r#"{"name": "same"}"#);
        let config = config_with_marketplace("same");

        assert!(declared_name_problems(&config, cache.path()).is_empty());
    }

    #[test]
    fn a_clone_that_is_not_synced_yet_is_not_a_problem() {
        let cache = tempfile::tempdir().unwrap();
        let config = config_with_marketplace("not-synced");

        assert!(declared_name_problems(&config, cache.path()).is_empty());
    }

    #[test]
    fn a_manifest_without_a_name_is_reported_not_skipped() {
        let cache = tempfile::tempdir().unwrap();
        write_manifest(cache.path(), "broken", r#"{"plugins": []}"#);
        let config = config_with_marketplace("broken");

        let problems = declared_name_problems(&config, cache.path());

        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("cannot read its manifest name"),
            "{problems:?}"
        );
    }
}
