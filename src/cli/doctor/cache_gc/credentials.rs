//! Drops the credential entries of the cache folders that GC removed (#1057).

use std::path::PathBuf;

/// Drop the platform credential entry belonging to each cache folder GC just
/// deleted, and report how many went (#1057).
///
/// Keyed by folder path, so this only ever touches entries llmenv's own folders
/// owned — never the user's default `~/.claude` login. Deliberately confined to
/// the explicit `llmenv clean` path: removing credential entries as a side effect
/// of `export` would be the wrong default.
pub(in crate::cli) fn forget_credentials_for(removed: &[PathBuf]) -> usize {
    removed
        .iter()
        .filter(|path| {
            crate::auth::credentials::forget(path)
                .inspect_err(|e| {
                    tracing::error!(
                        "could not drop credential entry for {}: {e}",
                        path.display()
                    );
                })
                .unwrap_or(false)
        })
        .count()
}
