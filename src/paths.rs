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

/// Non-unix targets have no `O_NOFOLLOW`; the name check remains the only guard there.
#[cfg(not(unix))]
pub(crate) fn refuse_symlink(opts: &mut std::fs::OpenOptions) -> &mut std::fs::OpenOptions {
    opts
}

/// File name used for SessionEnd dedup across hook run and memory CLI.
/// Shared between `hook_run` and `memory` modules — must not drift.
pub(crate) const HOOK_STORE_CHUNK: &str = "hook_store_chunk";
