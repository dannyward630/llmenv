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
/// Fails when `path` itself cannot be stat-ed or listed. A zero there would look like a real
/// measurement, so the failure is returned instead (#3038). An entry below `path` that cannot be
/// read is logged and left out of the total, so one unreadable subtree does not drop the whole
/// estimate.
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
    let mut total: u64 = 0;
    for entry in entries.filter_map(|e| crate::paths::readable_entry(path, "the size estimate", e))
    {
        match dir_size(&entry.path()) {
            Ok(bytes) => total = total.saturating_add(bytes),
            Err(e) => tracing::warn!("{e:#}; that entry is left out of the size estimate"),
        }
    }
    Ok(total)
}

#[cfg(all(test, unix))]
#[expect(
    clippy::expect_used,
    reason = "a test fails loudly when its fixture cannot be set up"
)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn an_unreadable_subdirectory_is_left_out_and_its_siblings_still_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("readable"), [0u8; 10]).expect("write fixture");
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).expect("create locked dir");
        std::fs::write(locked.join("hidden"), [0u8; 99]).expect("write hidden fixture");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("lock the directory");

        let total = dir_size(dir.path());

        // Restore access so the tempdir can be removed, whatever the assertion said.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700))
            .expect("unlock the directory");
        assert_eq!(total.expect("the root is readable"), 10);
    }
}
