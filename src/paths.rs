pub(crate) mod dirfd;

pub use llmenv_paths::{
    binary_on_path, config_dir, config_path, copy_replacing_symlink, create_dir_owner_only,
    cwd_under_prefix, expand_tilde, has_parent_component, is_unsafe_join_target,
    is_valid_short_name, read_dir_optional, refuse_symlink, reject_non_regular_file,
    resolve_in_path_list, resolve_on_path, state_dir, write_owner_only, write_owner_only_atomic,
};

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::refuse_symlink;

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

/// File name used for SessionEnd dedup across hook run and memory CLI.
/// Shared between `hook_run` and `memory` modules — must not drift.
pub(crate) const HOOK_STORE_CHUNK: &str = "hook_store_chunk";
