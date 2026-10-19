//! The check that each pinned marketplace clone sits on its pinned commit.

use std::path::Path;

use crate::config::Config;
use crate::plugins::cache;

/// Verify pinned marketplaces: when a marketplace source includes a `#ref`
/// pin, the checked-out HEAD should match that pinned ref. Use `^{commit}`
/// dereferencing so annotated tags don't false-positive (#695).
pub(super) fn report_pinned_marketplaces(config: &Config, cache_dir: &Path, warn: &str) {
    for m in &config.marketplace {
        let (_, pinned_ref) = cache::split_source_ref(&m.source);
        let Some(pinned_ref) = pinned_ref else {
            continue;
        };
        let mkt_path = cache::marketplace_path(cache_dir, &m.name);
        if !mkt_path.join(".git").exists() {
            continue;
        }
        let head = match cache::git_head(&mkt_path) {
            Ok(head) => head,
            Err(e) => {
                eprintln!(
                    "{warn} marketplace '{}': cannot read HEAD in its local clone: {e}",
                    m.name,
                );
                continue;
            }
        };
        let pinned_sha = match cache::git_peeled_ref(&mkt_path, pinned_ref) {
            Ok(sha) => sha,
            Err(e) => {
                eprintln!(
                    "{warn} marketplace '{}' pinned to '{}' cannot be resolved in the local \
                     clone: {e} — run `llmenv plugin-sync` to repair",
                    m.name, pinned_ref,
                );
                continue;
            }
        };
        if head != pinned_sha {
            eprintln!(
                "{warn} marketplace '{}' pinned to '{}': HEAD ({}) does not match \
                 the pinned ref's commit ({}) — run `llmenv plugin-sync` to repair",
                m.name,
                pinned_ref,
                &head[..head.len().min(7)],
                &pinned_sha[..pinned_sha.len().min(7)],
            );
        }
    }
}
