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
    let mut total: u64 = 0;
    // Not following links keeps a symlink sized by its own length, and stops the walk from
    // leaving the tree. The root link is not followed either, because walkdir follows it by default.
    for step in walkdir::WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
    {
        match step {
            Ok(entry) if entry.file_type().is_dir() => {}
            Ok(entry) => match entry.metadata() {
                Ok(meta) => total = total.saturating_add(meta.len()),
                Err(e) => skip_entry(&e, path)?,
            },
            // A missing root has no size. A missing entry below it was removed during the walk.
            Err(e) if e.depth() == 0 && is_not_found(&e) => return Ok(0),
            Err(e) => skip_entry(&e, path)?,
        }
    }
    Ok(total)
}

/// Fails for the root, because a zero there would look like a real measurement (#3038). Logs and
/// skips any entry below it, so one unreadable subtree does not drop the whole estimate.
fn skip_entry(err: &walkdir::Error, root: &Path) -> anyhow::Result<()> {
    if err.depth() == 0 {
        anyhow::bail!(
            "cannot read {} for the size estimate: {err}",
            root.display()
        );
    }
    if !is_not_found(err) {
        tracing::error!("{err}; that entry is left out of the size estimate");
    }
    Ok(())
}

fn is_not_found(err: &walkdir::Error) -> bool {
    err.io_error()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
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

    #[test]
    fn a_root_symlink_counts_its_own_length_and_its_target_is_not_walked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target");
        std::fs::create_dir(&target).expect("create target dir");
        std::fs::write(target.join("big"), [0u8; 99]).expect("write target fixture");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");
        let link_len = std::fs::symlink_metadata(&link).expect("stat link").len();

        let total = dir_size(&link).expect("the link is readable");

        assert_eq!(total, link_len);
    }

    #[test]
    fn a_symlink_counts_its_own_length_and_its_target_is_not_walked_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target");
        std::fs::create_dir(&target).expect("create target dir");
        std::fs::write(target.join("big"), [0u8; 99]).expect("write target fixture");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");
        let link_len = std::fs::symlink_metadata(&link).expect("stat link").len();

        let total = dir_size(dir.path()).expect("the root is readable");

        assert_eq!(total, 99 + link_len);
    }
}
