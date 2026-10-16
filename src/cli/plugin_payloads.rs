//! Payload lookup for external plugins during the export path. A plugin whose payload is not
//! usable is dropped with an agent notice, never served from the wrong place.

use super::*;

/// Tells the user and the agent that an external plugin is not loaded. A plugin with no install
/// path would load from the marketplace path, which does not hold its payload (#2750).
///
/// The cause is a fixed phrase. A lookup error can carry a plugin source URL, and that text
/// goes into the agent's context, so the error detail is logged by the caller, not here.
fn note_unloaded_plugin(plugin: &str, marketplace: &str, cause: &str) {
    let notice = format!(
        "llmenv: external plugin '{plugin}@{marketplace}' was not loaded: {cause}. Fix: run \
         `llmenv plugin-sync` to refresh the clone."
    );
    eprintln!("warning: {notice}");
    crate::agent_notice::record(notice);
}

/// Look up stable external plugin payload paths for resolved plugins. Non-refreshing
/// (export path): an external plugin whose payload is not usable is dropped and noted, so the
/// agent never sees an install path that points nowhere. Run `llmenv plugin-sync` to restore it.
pub(super) fn sync_plugin_payloads(
    cache_root: &Path,
    plugins: Vec<crate::plugins::resolve::ResolvedPlugin>,
) -> Vec<crate::plugins::resolve::ResolvedPlugin> {
    plugins
        .into_iter()
        .filter_map(|mut p| {
            let mkt_path = crate::plugins::cache::marketplace_path(cache_root, &p.marketplace);
            let entries = match crate::plugins::cache::read_marketplace_plugins(&mkt_path) {
                Ok(entries) => entries,
                Err(e) => {
                    // #893: read_marketplace_plugins now propagates real read
                    // errors (a missing manifest is Ok(empty)), so surface the
                    // cause rather than a fixed plugin-sync hint that won't fix
                    // a permission error or a corrupt manifest.
                    tracing::warn!(
                        "cannot read marketplace manifest for '{}': {e:#}",
                        p.marketplace
                    );
                    note_unloaded_plugin(
                        &p.plugin,
                        &p.marketplace,
                        "its marketplace manifest is unreadable",
                    );
                    return None;
                }
            };
            let Some(entry) = entries.iter().find(|e| e.name == p.plugin) else {
                // Kind is unknown without the entry, and an external plugin served from the clone
                // would load the wrong payload, so it is dropped rather than kept (#2780).
                tracing::warn!(
                    "plugin '{}' not found in marketplace '{}' manifest — verify plugin name, \
                     look for an earlier warning that skipped its entry, or run \
                     `llmenv plugin-sync` to refresh the clone",
                    p.plugin,
                    p.marketplace
                );
                note_unloaded_plugin(
                    &p.plugin,
                    &p.marketplace,
                    "it is missing from its marketplace manifest",
                );
                return None;
            };
            if !crate::plugins::cache::is_external_plugin_source(&entry.source) {
                return Some(p);
            }
            match crate::plugins::cache::sync_plugin_entry(cache_root, &p.marketplace, entry, false)
            {
                Ok(state) => {
                    p.install_path = Some(state.install_location.to_string_lossy().into_owned());
                    p.git_commit_sha = state.head;
                }
                Err(crate::plugins::cache::SyncError::NotCloned { .. }) => {
                    note_unloaded_plugin(&p.plugin, &p.marketplace, "it is not fetched yet");
                    return None;
                }
                Err(e) => {
                    tracing::warn!(
                        "payload lookup for '{}@{}' failed: {e:#}",
                        p.plugin,
                        p.marketplace
                    );
                    note_unloaded_plugin(&p.plugin, &p.marketplace, "its payload lookup failed");
                    return None;
                }
            }
            Some(p)
        })
        .collect()
}
