pub(crate) mod dirfd;

pub use llmenv_paths::{
    config_dir, config_path, create_dir_owner_only, cwd_under_prefix, expand_tilde,
    has_parent_component, is_unsafe_join_target, is_valid_short_name, read_dir_optional, state_dir,
    write_owner_only, write_owner_only_atomic,
};

/// Makes an open refuse a symlink at the final path component (#2599).
/// A name check before the open is not enough: a symlink swapped in after the check is followed,
/// and the write then lands on the symlink's target.
#[cfg(unix)]
pub(crate) fn refuse_symlink(opts: &mut std::fs::OpenOptions) -> &mut std::fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt as _;
    opts.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{follow_symlink, refuse_symlink, write_owner_only_atomic};

    #[test]
    fn refuse_symlink_refuses_a_symlink_and_opens_a_plain_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true);
        assert!(refuse_symlink(&mut opts).open(&link).is_err());
        assert!(refuse_symlink(&mut opts).open(&target).is_ok());
    }

    #[test]
    fn follow_symlink_returns_the_target_of_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let resolved = follow_symlink(&link).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&target).unwrap());
    }

    #[test]
    fn follow_symlink_returns_a_plain_or_missing_path_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain");
        std::fs::write(&plain, b"").unwrap();
        let missing = dir.path().join("missing");
        assert_eq!(follow_symlink(&plain).unwrap(), plain);
        assert_eq!(follow_symlink(&missing).unwrap(), missing);
    }

    #[test]
    fn follow_symlink_fails_on_a_dangling_link() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path().join("gone"), &link).unwrap();
        assert!(follow_symlink(&link).is_err());
    }

    #[test]
    fn a_write_through_the_followed_path_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, b"old").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write_owner_only_atomic(&follow_symlink(&link).unwrap(), b"new").unwrap();
        assert!(link.is_symlink(), "the write must not replace the link");
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }
}

/// Non-unix targets have no `O_NOFOLLOW`; the name check remains the only guard there.
#[cfg(not(unix))]
pub(crate) fn refuse_symlink(opts: &mut std::fs::OpenOptions) -> &mut std::fs::OpenOptions {
    opts
}

/// Create `path` for writing, truncating it, and refuse a symlink at `path` (#2686). A file at a
/// fixed name in a directory the user writes to would otherwise take its write through a link.
///
/// # Errors
/// `path` is a symlink, or the open fails.
pub(crate) fn create_refusing_symlink(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    refuse_symlink(
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true),
    )
    .open(path)
}

/// Return the file that `path` names, with a symlink followed to its target.
///
/// A dotfiles-managed file is a symlink on purpose. An atomic rename over the link name replaces
/// the link with a plain file and leaves the target stale (#2883), so a write goes to the target.
/// A path that is not a symlink, or that does not exist yet, comes back unchanged.
///
/// # Errors
/// `path` is a symlink whose target cannot be resolved, such as a dangling link.
pub(crate) fn follow_symlink(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::canonicalize(path),
        Ok(_) => Ok(path.to_path_buf()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(e) => Err(e),
    }
}

/// File name used for SessionEnd dedup across hook run and memory CLI.
/// Shared between `hook_run` and `memory` modules — must not drift.
pub(crate) const HOOK_STORE_CHUNK: &str = "hook_store_chunk";

/// Keeps a readable directory entry. An unreadable one is logged with its directory and
/// `purpose`, then skipped, so the loss is visible (#2718).
pub(crate) fn readable_entry(
    dir: &std::path::Path,
    purpose: &str,
    entry: std::io::Result<std::fs::DirEntry>,
) -> Option<std::fs::DirEntry> {
    entry
        .inspect_err(|e| {
            tracing::error!(
                error = %e,
                dir = %dir.display(),
                "cannot read a directory entry for {purpose}"
            );
        })
        .ok()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::expect_fun_call)]
mod readable_entry_tests {
    use super::readable_entry;

    #[test]
    fn an_unreadable_entry_is_dropped_and_a_readable_one_is_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("a")).expect("create entry");
        let entry = std::fs::read_dir(dir.path())
            .expect("read dir")
            .next()
            .expect("one entry");
        assert!(readable_entry(dir.path(), "test", entry).is_some());
        let err = Err(std::io::Error::other("boom"));
        assert!(readable_entry(dir.path(), "test", err).is_none());
    }
}
