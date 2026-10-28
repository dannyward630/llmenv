//! On-disk size of a cache or temp tree, used by the status line and the temp sweep (#3038).

use std::path::Path;

/// Recursively sum the on-disk size of every entry under `path`.
///
/// `prune`'s `removed` list holds directory (or symlink) paths, not files —
/// stat'ing the directory entry itself would report a few KiB regardless of
/// the tree's actual contents, so this walks in and sums real file sizes.
/// Symlinks are sized by their own on-disk length rather than followed,
/// matching `prune`'s own care not to traverse through them. Unreadable
/// entries are skipped (best-effort estimate, not exact accounting).
///
/// # Errors
/// Fails when the path cannot be stat-ed or a directory cannot be listed. A zero would look like
/// a real measurement, so the failure is returned instead (#3038).
pub(crate) fn dir_size(path: &Path) -> anyhow::Result<u64> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        // A missing path (an absent cache root, or an entry removed during the walk) has no size.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => anyhow::bail!("cannot stat {} for the size estimate: {e}", path.display()),
    };
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    let entries = std::fs::read_dir(path).map_err(|e| {
        anyhow::anyhow!("cannot list {} for the size estimate: {e}", path.display())
    })?;
    entries
        .filter_map(|entry| crate::paths::readable_entry(path, "the size estimate", entry))
        .map(|entry| dir_size(&entry.path()))
        .sum()
}
