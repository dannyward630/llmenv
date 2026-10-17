//! Replace the running llmenv binary with a downloaded release.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Copy `src` to `dst` like `std::fs::copy`, with the destination opened by
/// [`crate::paths::create_refusing_symlink`]. The copy keeps the source mode, because the backup
/// is the binary that a failed upgrade restores.
fn copy_refusing_symlink(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    let mut input = std::fs::File::open(src)?;
    let mut output = crate::paths::create_refusing_symlink(dst)?;
    std::io::copy(&mut input, &mut output)?;
    output.set_permissions(input.metadata()?.permissions())
}

/// Install `data` as the new binary, with backup and restore on failure.
pub(super) fn install_binary(data: &[u8]) -> Result<()> {
    // Replace target is this binary's own path. nosemgrep: rust.lang.security.current-exe.current-exe
    let current_exe = std::env::current_exe().context("failed to get current executable path")?;
    let current_exe = resolve_install_path(&current_exe)?;
    let current_dir = current_exe
        .parent()
        .context("current executable has no parent directory")?;

    // Backup lives next to the current binary (same filesystem for atomic rename)
    let backup = current_dir.join(".llmenv-upgrade.bak");
    copy_refusing_symlink(&current_exe, &backup)
        .with_context(|| format!("failed to backup current binary to {}", backup.display()))?;

    // Write new binary to a temp file in the same directory
    let temp = current_dir.join(".llmenv-upgrade.new");
    let write_result = (|| -> Result<()> {
        let mut tmp = crate::paths::create_refusing_symlink(&temp)
            .context("failed to create temp file for new binary")?;
        tmp.write_all(data).context("failed to write new binary")?;
        tmp.sync_all().context("failed to sync new binary")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            tmp.set_permissions(perms)
                .context("failed to set executable permissions")?;
        }

        // Rename over the current binary
        std::fs::rename(&temp, &current_exe).context("failed to replace current binary")?;
        Ok(())
    })();

    if let Err(e) = write_result {
        remove_leftover(&temp, "temp file");
        // Restore backup before propagating the error
        let restore_err = restore_backup(&current_exe, &backup);
        if let Err(re) = restore_err {
            anyhow::bail!("failed to install upgrade: {e}; AND failed to restore backup: {re}");
        }
        return Err(e.context("upgrade installation failed; backup restored"));
    }

    // Verify the new binary works
    match Command::new(&current_exe).arg("--version").output() {
        Ok(output) if output.status.success() => {
            remove_leftover(&backup, "backup");
            Ok(())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let restore_err = restore_backup(&current_exe, &backup).inspect_err(|e| {
                tracing::error!("upgrade: failed to restore backup after verification failure: {e}")
            });
            if let Err(re) = restore_err {
                anyhow::bail!(
                    "new binary failed verification (stderr: {stderr}); AND failed to restore backup: {re}"
                );
            }
            anyhow::bail!("new binary failed verification (stderr: {stderr}); restored original");
        }
        Err(e) => {
            let restore_err = restore_backup(&current_exe, &backup).inspect_err(|e| {
                tracing::error!("upgrade: failed to restore backup after verification error: {e}")
            });
            if let Err(re) = restore_err {
                anyhow::bail!(
                    "could not verify new binary: {e}; AND failed to restore backup: {re}"
                );
            }
            anyhow::bail!("could not verify new binary: {e}; restored original");
        }
    }
}

/// Remove a file that the upgrade left behind. A failure is logged at error level, because
/// the default log filter hides warn. A silent leftover binary is a failure the user never
/// sees (#2666).
pub(super) fn remove_leftover(path: &Path, what: &str) {
    if let Err(e) = std::fs::remove_file(path) {
        tracing::error!("upgrade: failed to remove {what} {}: {e}", path.display());
    }
}

fn restore_backup(target: &Path, backup: &Path) -> Result<()> {
    std::fs::rename(backup, target).context("failed to restore backup binary")
}

/// Resolve the running binary to the file the upgrade replaces.
///
/// The backup, the temp file, and the rename all use this path. A symlink must resolve first,
/// or the rename replaces the link and the real file keeps the old binary. A writable install
/// directory already lets its owner replace the binary, so resolving adds no privilege.
fn resolve_install_path(exe: &Path) -> Result<std::path::PathBuf> {
    std::fs::canonicalize(exe).with_context(|| format!("failed to resolve {}", exe.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_symlinked_install_path_resolves_to_the_real_binary() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let real = dir.path().join("llmenv-real");
        std::fs::write(&real, b"binary").expect("write the real binary");
        let link = dir.path().join("llmenv");
        std::os::unix::fs::symlink(&real, &link).expect("create the symlink");
        let resolved = resolve_install_path(&link).expect("a symlink resolves");
        assert_eq!(
            resolved,
            std::fs::canonicalize(&real).expect("canonical real path")
        );
    }

    #[test]
    fn a_missing_install_path_is_an_error_that_names_it() {
        let missing = Path::new("/nonexistent/llmenv-install-path");
        let err = resolve_install_path(missing).expect_err("a missing path must not resolve");
        assert!(
            err.to_string().contains("/nonexistent/llmenv-install-path"),
            "{err:#}"
        );
    }
}
