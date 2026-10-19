//! The check that each pinned marketplace clone sits on its pinned commit.

use std::path::Path;

use crate::config::Config;
use crate::plugins::cache;

#[cfg(test)]
mod tests;

/// Verify pinned marketplaces: when a marketplace source includes a `#ref`
/// pin, the checked-out HEAD should match that pinned ref. Use `^{commit}`
/// dereferencing so annotated tags don't false-positive (#695).
pub(super) fn report_pinned_marketplaces(config: &Config, cache_dir: &Path, warn: &str) {
    for m in &config.marketplace {
        if let Some(problem) = pin_problem(&m.name, &m.source, cache_dir) {
            eprintln!("{warn} {problem}");
        }
    }
}

/// The problem with one marketplace pin, as a message without the warning prefix.
/// `None` when the source has no pin, or when the pin holds.
fn pin_problem(name: &str, source: &str, cache_dir: &Path) -> Option<String> {
    let (_, pinned_ref) = cache::split_source_ref(source);
    let pinned_ref = pinned_ref?;
    let mkt_path = cache::marketplace_path(cache_dir, name);
    // #1436 variant: an unstattable clone must not silently skip the pin
    // check — that reads as "pin verified" in the output.
    match mkt_path.join(".git").try_exists() {
        Ok(true) => {}
        Ok(false) => return None,
        Err(e) => {
            return Some(format!(
                "marketplace '{name}' pin not verified — cannot check its clone at {}: {e}",
                mkt_path.display(),
            ));
        }
    }
    let head = match cache::git_head(&mkt_path) {
        Ok(head) => head,
        Err(e) => {
            return Some(format!(
                "marketplace '{name}': cannot read HEAD in its local clone: {e}"
            ));
        }
    };
    let pinned_sha = match cache::git_peeled_ref(&mkt_path, pinned_ref) {
        Ok(sha) => sha,
        Err(e) => {
            return Some(format!(
                "marketplace '{name}' pinned to '{pinned_ref}' cannot be resolved in the local \
                 clone: {e} — run `llmenv plugin-sync` to repair"
            ));
        }
    };
    (head != pinned_sha).then(|| {
        format!(
            "marketplace '{name}' pinned to '{pinned_ref}': HEAD ({}) does not match \
             the pinned ref's commit ({}) — run `llmenv plugin-sync` to repair",
            &head[..head.len().min(7)],
            &pinned_sha[..pinned_sha.len().min(7)],
        )
    })
}
