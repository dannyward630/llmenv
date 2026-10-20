//! Owner-only folders and files.

use std::path::Path;

/// Create a directory (and any missing parent components) with owner-only
/// permissions (mode 0o700 on Unix) from the moment of creation, and harden
/// it to 0o700 if it already existed at a looser mode. On Windows falls back
/// to `create_dir_all`'s default permissions.
///
/// Use instead of `create_dir_all` followed by a separate `set_permissions`
/// call for any directory that must never be world-readable — the two-call
/// version leaves the directory at the umask default (typically 0o755)
/// between creation and the chmod, a TOCTOU window (#1113), and a caller who
/// skips the follow-up chmod (or an older llmenv version, before this
/// hardening existed) leaves it world-readable indefinitely (#1178).
///
/// # Errors
/// Propagates directory-creation failure, and failure to chmod an
/// already-existing directory (e.g. owned by another user).
pub fn create_dir_owner_only(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        // DirBuilder's mode only governs directories it creates. If `dir`
        // already existed (an older llmenv version, a caller that used a
        // bare create_dir_all, a permissive umask), its mode is left
        // untouched -- self-heal it so every caller gets an owner-only
        // directory regardless of whether it was just created.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Write `content` to `path` with owner-only permissions (mode 0o600) on Unix.
/// On Windows falls back to default permissions. Creates the file if absent,
/// truncates if present. Use for any file containing user state or
/// credentials (settings, sync state, MCP configs, ICM memory) where
/// world-readable defaults would leak data on shared systems.
///
/// A FIFO or another special file at `path` is an error on Unix, and the open does not block on it.
/// A symlink at `path` is an error on Unix (`O_NOFOLLOW`), so the write never lands on the link's
/// target (#2686). Only the final component is checked. A symlink in a parent directory is still
/// followed, so the parent must already be a directory the user controls.
pub fn write_owner_only(path: &Path, content: &[u8]) -> std::io::Result<()> {
    write_owner_only_unnamed(path, content, false)
        .map_err(|e| std::io::Error::new(e.kind(), format!("cannot write {}: {e}", path.display())))
}

/// Write `content` as [`write_owner_only`] does, but with the owner executable bit (mode 0o700)
/// on Unix, for a script that a skill or a hook runs directly. The mode is set on the open file,
/// so a file that exists at mode 0o600 gets the bit too. Off Unix this is [`write_owner_only`].
///
/// # Errors
/// As [`write_owner_only`], and a failure to set the mode.
pub fn write_owner_only_executable(path: &Path, content: &[u8]) -> std::io::Result<()> {
    write_owner_only_unnamed(path, content, true)
        .map_err(|e| std::io::Error::new(e.kind(), format!("cannot write {}: {e}", path.display())))
}

/// Write `content` as [`write_owner_only`] does, with the executable bit when the file at `source`
/// has one, so a script copied from a bundle or a skill can still run directly.
///
/// # Errors
/// `source` cannot be read for its metadata, or the write fails.
pub fn write_owner_only_like(path: &Path, content: &[u8], source: &Path) -> std::io::Result<()> {
    if is_executable(source)? {
        write_owner_only_executable(path, content)
    } else {
        write_owner_only(path, content)
    }
}

/// Whether the file at `path` has an executable bit, following a symlink. Off Unix a file has no
/// such bit, so the answer is false.
///
/// # Errors
/// The file cannot be read for its metadata.
pub fn is_executable(path: &Path) -> std::io::Result<bool> {
    let meta = std::fs::metadata(path).map_err(|e| {
        std::io::Error::new(e.kind(), format!("cannot read {}: {e}", path.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        Ok(false)
    }
}

fn write_owner_only_unnamed(path: &Path, content: &[u8], executable: bool) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = if executable { 0o700 } else { 0o600 };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK)
                    .bits()
                    .cast_signed(),
            )
            .open(path)?;
        // A FIFO with no reader fails the open with ENXIO; one with a reader fails here (#2942).
        crate::check_regular_file(&file)?;
        if executable {
            // The open mode applies only to a new file; an existing one keeps its old mode.
            file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        file.write_all(content)?;
    }
    #[cfg(not(unix))]
    {
        let _ = executable;
        std::fs::write(path, content)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn create_dir_owner_only_is_0o700_from_creation() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("nested").join("store");
        create_dir_owner_only(&dir).unwrap();

        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "dir must be born owner-only, got {mode:o}");
    }

    #[test]
    fn create_dir_owner_only_is_idempotent_on_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("store");
        create_dir_owner_only(&dir).unwrap();
        create_dir_owner_only(&dir).unwrap();
        assert!(dir.is_dir());
    }

    // A directory created before this hardening shipped (older llmenv
    // version, a caller using bare create_dir_all, a permissive umask) must
    // still end up owner-only the next time something calls
    // create_dir_owner_only on it -- not just directories it creates fresh.
    #[cfg(unix)]
    #[test]
    fn create_dir_owner_only_hardens_a_preexisting_looser_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("store");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        create_dir_owner_only(&dir).unwrap();

        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o700,
            "pre-existing looser dir must be hardened, got {mode:o}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_owner_only_sets_mode_0o600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("secret");
        write_owner_only(&path, b"sensitive").expect("write");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        // Group/other bits must be clear — file is owner-only.
        assert_eq!(mode & 0o077, 0, "group/other bits set: {mode:o}");
        let body = std::fs::read(&path).expect("read");
        assert_eq!(body, b"sensitive");
    }

    #[cfg(unix)]
    #[test]
    fn write_owner_only_refuses_a_symlink_and_leaves_its_target_alone() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let target = tmp.path().join("target");
        let link = tmp.path().join("link");
        std::fs::write(&target, b"keep").expect("seed target");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let err = write_owner_only(&link, b"attacker").expect_err("symlink must be refused");
        let eloop = std::io::Error::from_raw_os_error(rustix::io::Errno::LOOP.raw_os_error());
        assert_eq!(
            err.kind(),
            eloop.kind(),
            "a symlink at the final component must fail with ELOOP: {err}"
        );
        assert!(
            err.to_string().contains(&link.display().to_string()),
            "{err}"
        );
        assert_eq!(std::fs::read(&target).expect("read target"), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn write_owner_only_fails_at_once_on_a_fifo() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        assert!(write_owner_only(&path, b"x").is_err());
    }

    #[cfg(unix)]
    #[cfg(unix)]
    #[test]
    fn the_executable_writer_sets_0o700_on_a_new_and_an_existing_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("run.sh");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        write_owner_only_executable(&path, b"a").unwrap();
        assert_eq!(mode(&path), 0o700);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_owner_only_executable(&path, b"b").unwrap();
        assert_eq!(mode(&path), 0o700, "an existing file gets the bit");
        assert_eq!(std::fs::read(&path).unwrap(), b"b");
    }

    #[cfg(unix)]
    #[test]
    fn the_like_writer_takes_the_executable_bit_from_its_source() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let script = tmp.path().join("src.sh");
        std::fs::write(&script, b"x").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o750)).unwrap();
        let plain = tmp.path().join("src.md");
        std::fs::write(&plain, b"x").unwrap();
        assert!(is_executable(&script).unwrap() && !is_executable(&plain).unwrap());
        write_owner_only_like(&tmp.path().join("a"), b"x", &script).unwrap();
        write_owner_only_like(&tmp.path().join("b"), b"x", &plain).unwrap();
        assert_eq!(mode(&tmp.path().join("a")), 0o700);
        assert_eq!(mode(&tmp.path().join("b")), 0o600);
        let err = is_executable(&tmp.path().join("missing")).unwrap_err();
        assert!(err.to_string().contains("missing"), "{err}");
    }

    #[test]
    fn write_owner_only_truncates_existing_file() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("file");
        write_owner_only(&path, b"longer content").expect("write1");
        write_owner_only(&path, b"short").expect("write2");
        let body = std::fs::read(&path).expect("read");
        assert_eq!(body, b"short");
    }
}
