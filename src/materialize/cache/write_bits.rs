//! Owner write-bit repair for the cache tree, so a GC can remove a folder llmenv made.

use std::path::Path;

/// `remove_dir_all`, retried once after restoring the owner write bit on any
/// directory in the tree that lacks it.
///
/// `remove_dir_all` unlinks children *through* their parent directory, so a
/// directory with no owner write bit (a `0o555` left behind by some other tool
/// that used the cache as scratch space) makes its whole subtree undeletable
/// and fails with `PermissionDenied`. llmenv created that tree, so a cache GC
/// restoring write access to its own directories is expected behavior, not an
/// escalation — without it the cache grows forever with no way for the tool to
/// clean up after itself.
pub(super) fn remove_dir_all_forcing_writable(p: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(p) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // Best-effort: if the chmod pass can't fix it either, the retry
            // surfaces the real error rather than the chmod's.
            restore_dir_write_bits(p);
            std::fs::remove_dir_all(p)
        }
        other => other,
    }
}

/// Add the owner `rwx` bits to `dir` and every directory beneath it.
///
/// Symlinks are never followed and never chmod'd, so a link planted inside the
/// cache can't be used to widen permissions on a target outside it — the walk
/// stays within the tree it was handed. Errors are logged, not returned: this is
/// a best-effort assist for the retry in [`remove_dir_all_forcing_writable`],
/// and the retry reports the failure that actually matters.
pub(super) fn restore_dir_write_bits(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let meta = match std::fs::symlink_metadata(dir) {
            Ok(meta) => meta,
            // A missing folder has nothing to restore, so it stays silent.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                tracing::error!(
                    "cannot read {} for cache cleanup: {e}. Fix: check the cache folder's \
                     permissions; its contents may not be removed.",
                    dir.display()
                );
                return;
            }
        };
        if !meta.is_dir() {
            return;
        }
        let mode = meta.permissions().mode();
        // 0o700: traverse + write + read, the minimum needed to unlink children.
        if mode & 0o700 != 0o700
            && let Err(e) =
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode | 0o700))
        {
            tracing::error!(
                "cannot make {} writable for cache cleanup: {e}; cleanup of its contents may fail",
                dir.display()
            );
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!("cannot list {} during cache cleanup: {e}", dir.display());
                return;
            }
        };
        let readable =
            entries.filter_map(|entry| crate::paths::readable_entry(dir, "cache cleanup", entry));
        for entry in readable {
            // `file_type()` is lstat-equivalent: a symlink reports as a symlink
            // rather than as the directory it points at.
            // An unreadable type is not a directory, so the write bits would stay unrestored.
            // The error is logged with its path, and the cleanup fails at the next step.
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => restore_dir_write_bits(&entry.path()),
                Ok(_) => {}
                Err(e) => tracing::error!(
                    "cache cleanup cannot read the type of {}: {e}",
                    entry.path().display()
                ),
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::expect_used, reason = "a test: a failed setup must stop it")]
mod tests {
    use super::*;
    use crate::test_log_capture::capture_logs;

    #[test]
    fn a_path_that_cannot_be_read_logs_the_cause_and_a_fix() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("plain-file");
        std::fs::write(&file, "x").expect("write a test file");
        // A file used as a folder makes `symlink_metadata` fail with a non-NotFound error.
        let under_a_file = file.join("child");
        let logs = capture_logs(|| restore_dir_write_bits(&under_a_file));
        assert!(
            logs.contains("cannot read"),
            "the log must name the failure: {logs}"
        );
        assert!(logs.contains("Fix:"), "the log must name a fix: {logs}");
    }

    #[test]
    fn a_missing_folder_has_nothing_to_restore_and_logs_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let logs = capture_logs(|| restore_dir_write_bits(&dir.path().join("absent")));
        assert!(logs.is_empty(), "a missing folder must stay silent: {logs}");
    }
}
