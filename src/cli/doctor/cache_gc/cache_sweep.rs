//! The sweep of the hashed cache, and the prune of marketplace clones that no config entry names.
//!
//! `cache::gc` ages the whole `marketplaces` folder as one unit, so a clone left behind by a
//! renamed marketplace would stay until the folder itself is old (#3084).

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use super::GcOutput;
use super::credentials::report_credential_drops;
use crate::config::{CONTEXT_MODE_MARKETPLACE, Config, split_plugin_ref};
use crate::materialize::cache::{GcReport, gc_remove_dir, gc_remove_link};

/// Sweep the hashed cache under `cache_dir`, then the orphan marketplace clones. Returns the count
/// of failed entries.
pub(super) fn sweep_cache(
    cache_dir: &Path,
    config: &Config,
    retention: Duration,
    out: GcOutput<'_>,
) -> usize {
    match crate::materialize::cache::gc(cache_dir, retention, config.cache.hashing, out.dry_run) {
        Ok(report) => {
            out.report("", &report);
            // A dry run deletes nothing, so it drops no credentials and has none to fail (#3073).
            let credential_failures = if out.dry_run {
                0
            } else {
                report_credential_drops(&report.removed, out)
            };
            let orphans = prune_orphan_marketplaces(cache_dir, config, out.dry_run);
            out.report(" (marketplace clones)", &orphans);
            report.failed.len() + credential_failures + orphans.failed.len()
        }
        Err(e) => {
            eprintln!("{} GC failed: {e:#}", out.warn);
            1
        }
    }
}

/// Clone names that some config entry still needs: a declared marketplace, the marketplace of a
/// plugin reference, and the context-mode marketplace, which the built-in feature uses.
fn referenced_marketplace_names(config: &Config) -> HashSet<&str> {
    let mut names: HashSet<&str> = config.marketplace.iter().map(|m| m.name.as_str()).collect();
    names.extend(
        config
            .plugin_collection
            .iter()
            .flat_map(|c| c.plugins.iter())
            .filter_map(|p| split_plugin_ref(p))
            .map(|(marketplace, _)| marketplace),
    );
    names.insert(CONTEXT_MODE_MARKETPLACE);
    names
}

/// Remove each clone under the marketplace folder that no config entry names. A dry run records
/// each one and deletes nothing.
fn prune_orphan_marketplaces(cache_dir: &Path, config: &Config, dry_run: bool) -> GcReport {
    let root = crate::plugins::cache::marketplace_cache_root(cache_dir);
    let mut report = GcReport::new(dry_run);
    let entries = match crate::paths::read_dir_optional(&root) {
        Ok(Some(entries)) => entries,
        Ok(None) => return report,
        Err(e) => {
            report.record_failure(root, format!("{e:#}"));
            return report;
        }
    };
    let keep = referenced_marketplace_names(config);
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                report.record_failure(root.clone(), format!("{e:#}"));
                continue;
            }
        };
        let path = entry.path();
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| keep.contains(name))
        {
            report.kept += 1;
            continue;
        }
        match entry.file_type() {
            Ok(ft) if ft.is_symlink() => gc_remove_link(&path, &mut report),
            Ok(ft) if ft.is_dir() => gc_remove_dir(&path, &mut report),
            Ok(_) => report.kept += 1,
            Err(e) => report.record_failure(path, format!("{e:#}")),
        }
    }
    report
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: a failed setup step should abort the test"
)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::config::{Marketplace, PluginCollection};

    fn config_with(marketplaces: &[&str], plugins: &[&str]) -> Config {
        Config {
            marketplace: marketplaces
                .iter()
                .map(|name| Marketplace {
                    name: (*name).into(),
                    source: "https://example.com".into(),
                })
                .collect(),
            plugin_collection: vec![PluginCollection {
                name: "core".into(),
                when: vec![],
                plugins: plugins.iter().map(|p| (*p).into()).collect(),
            }],
            ..Config::default()
        }
    }

    fn make_clone(cache_dir: &Path, name: &str) -> std::path::PathBuf {
        let clone = crate::plugins::cache::marketplace_path(cache_dir, name);
        fs::create_dir_all(&clone).unwrap();
        fs::write(clone.join("marker"), b"x").unwrap();
        clone
    }

    #[test]
    fn an_orphan_clone_is_removed_and_every_referenced_clone_is_kept() {
        let cache = tempfile::tempdir().unwrap();
        let orphan = make_clone(cache.path(), "old-name");
        let declared = make_clone(cache.path(), "used");
        let via_plugin = make_clone(cache.path(), "via");
        let context = make_clone(cache.path(), CONTEXT_MODE_MARKETPLACE);
        let config = config_with(&["used"], &["via:some-plugin"]);

        let report = prune_orphan_marketplaces(cache.path(), &config, false);

        assert_eq!(report.removed, vec![orphan.clone()]);
        assert!(!orphan.exists(), "the orphan clone must be removed");
        assert!(declared.exists() && via_plugin.exists() && context.exists());
    }

    #[test]
    fn a_dry_run_lists_an_orphan_clone_and_deletes_nothing() {
        let cache = tempfile::tempdir().unwrap();
        let orphan = make_clone(cache.path(), "old-name");
        let config = config_with(&[], &[]);

        let report = prune_orphan_marketplaces(cache.path(), &config, true);

        assert_eq!(report.removed, vec![orphan.clone()]);
        assert!(orphan.exists(), "a dry run must not delete");
    }

    #[test]
    fn a_missing_marketplace_folder_is_nothing_to_prune() {
        let cache = tempfile::tempdir().unwrap();
        let config = config_with(&[], &[]);

        let report = prune_orphan_marketplaces(cache.path(), &config, false);

        assert!(report.removed.is_empty());
        assert!(report.failed.is_empty());
    }
}
