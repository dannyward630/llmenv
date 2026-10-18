//! Replace the running llmenv binary with a downloaded release.

use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Install `data` as the new binary. The original stays in a backup until the new binary runs.
pub(super) fn install_binary(data: &[u8]) -> Result<()> {
    // The replace target is this binary's own path. nosemgrep: rust.lang.security.current-exe.current-exe
    let exe = std::env::current_exe().context("failed to get current executable path")?;
    let current_exe = resolve_install_path(&exe)?;
    let current_dir = current_exe
        .parent()
        .context("current executable has no parent directory")?;

    // The backup and the temp file sit beside the binary, so the rename stays on one filesystem.
    let backup = current_dir.join(".llmenv-upgrade.bak");
    backup_current_binary(&current_exe, &backup)?;

    let temp = current_dir.join(".llmenv-upgrade.new");
    if let Err(e) = stage_binary(&temp, data) {
        remove_leftover(&temp, "temp file");
        remove_leftover(&backup, "backup");
        return Err(e.context(format!(
            "upgrade not installed; {} is unchanged",
            current_exe.display()
        )));
    }

    // The rename is the only step that changes the installed binary. When it fails, the
    // original is still in place, so no restore is needed.
    if let Err(e) = std::fs::rename(&temp, &current_exe) {
        remove_leftover(&temp, "temp file");
        remove_leftover(&backup, "backup");
        return Err(anyhow::Error::from(e).context(format!(
            "failed to replace {}; the original binary is unchanged",
            current_exe.display()
        )));
    }
    verify_installed(&current_exe, &backup)
}

/// Copy `current_exe` to `backup` before installing the new binary over it.
///
/// `copy_replacing_symlink`, not `std::fs::copy`: `backup` is a fixed,
/// predictable filename (`.llmenv-upgrade.bak`) in the binary's own install
/// directory, so a plain copy would write through a pre-existing symlink
/// planted there instead of replacing it — the same TOCTOU class
/// `src/materialize/inherit.rs` was hardened against for #1341, and
/// `src/materialize/mod.rs`'s `write_in_place`/`claude_code`'s `materialize`
/// were fixed for in #1422/#1423.
pub(super) fn backup_current_binary(current_exe: &Path, backup: &Path) -> Result<()> {
    crate::paths::copy_replacing_symlink(current_exe, backup)
        .with_context(|| format!("failed to backup current binary to {}", backup.display()))
}

/// Write `data` to `temp` with the executable mode, and flush it to disk.
fn stage_binary(temp: &Path, data: &[u8]) -> Result<()> {
    let mut tmp = crate::paths::create_refusing_symlink(temp)
        .with_context(|| format!("failed to create {}", temp.display()))?;
    tmp.write_all(data)
        .with_context(|| format!("failed to write {}", temp.display()))?;
    tmp.sync_all()
        .with_context(|| format!("failed to sync {}", temp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.set_permissions(std::fs::Permissions::from_mode(0o755))
            .with_context(|| {
                format!("failed to set executable permissions on {}", temp.display())
            })?;
    }
    Ok(())
}

/// Run the installed binary. A run that fails puts the backup back over it.
fn verify_installed(current_exe: &Path, backup: &Path) -> Result<()> {
    let failure = match Command::new(current_exe).arg("--version").output() {
        Ok(output) if output.status.success() => {
            remove_leftover(backup, "backup");
            return Ok(());
        }
        Ok(output) => format!(
            "new binary failed verification ({}; stderr: {})",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(e) => format!("could not run new binary to verify it: {e}"),
    };
    bail!("{}", restore_original(current_exe, backup, &failure))
}

/// Put the backup back over the installed binary, and describe the outcome for the user.
fn restore_original(current_exe: &Path, backup: &Path, failure: &str) -> String {
    match std::fs::rename(backup, current_exe) {
        Ok(()) => format!("{failure}; restored original"),
        Err(e) => {
            tracing::error!("upgrade: failed to restore {}: {e}", backup.display());
            format!(
                "{failure}; AND could not restore the original from {}: {e}. \
                 The unverified new binary is still at {}",
                backup.display(),
                current_exe.display()
            )
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

/// Resolve the running binary to the file the upgrade replaces.
///
/// The backup, the temp file, and the rename all use this path. A symlink must resolve first,
/// or the rename replaces the link and the real file keeps the old binary. A writable install
/// directory already lets its owner replace the binary, so resolving adds no privilege.
fn resolve_install_path(exe: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(exe).with_context(|| format!("failed to resolve {}", exe.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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

    /// A temp dir with an installed binary that links to `program`, and a backup that holds
    /// `old`. A link to a system program avoids writing an executable, which can fail with
    /// "text file busy" when another test forks at the same time.
    #[cfg(unix)]
    fn install_linked_to(program: &str, old: &[u8]) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let exe = dir.path().join("llmenv");
        std::os::unix::fs::symlink(program, &exe).unwrap();
        let backup = dir.path().join(".llmenv-upgrade.bak");
        std::fs::write(&backup, old).unwrap();
        (dir, exe, backup)
    }

    #[cfg(unix)]
    #[test]
    fn a_new_binary_that_runs_is_kept_and_the_backup_is_removed() {
        let (_dir, exe, backup) = install_linked_to("/usr/bin/true", b"old binary");
        verify_installed(&exe, &backup).unwrap();
        assert!(!backup.exists(), "the backup must be removed");
        assert!(exe.is_symlink(), "the new binary must stay in place");
    }

    #[cfg(unix)]
    #[test]
    fn a_new_binary_that_fails_is_replaced_by_the_backup() {
        let (_dir, exe, backup) = install_linked_to("/usr/bin/false", b"old binary");
        let err = verify_installed(&exe, &backup).unwrap_err().to_string();
        assert!(err.contains("failed verification (exit status: 1"), "{err}");
        assert!(err.ends_with("restored original"), "{err}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"old binary");
        assert!(!backup.exists(), "the backup must move back into place");
    }

    #[cfg(unix)]
    #[test]
    fn a_new_binary_that_cannot_start_is_replaced_by_the_backup() {
        let dir = tempfile::TempDir::new().unwrap();
        let exe = dir.path().join("llmenv");
        std::fs::write(&exe, b"not a program").unwrap();
        let backup = dir.path().join(".llmenv-upgrade.bak");
        std::fs::write(&backup, b"old binary").unwrap();
        let err = verify_installed(&exe, &backup).unwrap_err().to_string();
        assert!(
            err.contains("could not run new binary to verify it"),
            "{err}"
        );
        assert!(err.ends_with("restored original"), "{err}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"old binary");
    }

    #[test]
    fn a_failed_restore_names_the_backup_and_the_installed_binary() {
        let dir = tempfile::TempDir::new().unwrap();
        let exe = dir.path().join("llmenv");
        std::fs::write(&exe, b"new binary").unwrap();
        let backup = dir.path().join("missing.bak");
        let msg = restore_original(&exe, &backup, "new binary failed verification");
        assert!(
            msg.starts_with("new binary failed verification; AND"),
            "{msg}"
        );
        assert!(msg.contains(&backup.display().to_string()), "{msg}");
        assert!(
            msg.contains(&format!("still at {}", exe.display())),
            "{msg}"
        );
        assert_eq!(std::fs::read(&exe).unwrap(), b"new binary");
    }

    #[cfg(unix)]
    #[test]
    fn staging_writes_the_data_with_the_executable_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let temp = dir.path().join(".llmenv-upgrade.new");
        stage_binary(&temp, b"new binary").unwrap();
        assert_eq!(std::fs::read(&temp).unwrap(), b"new binary");
        let mode = std::fs::metadata(&temp).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn staging_refuses_a_symlink_at_the_temp_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, b"keep").unwrap();
        let temp = dir.path().join(".llmenv-upgrade.new");
        std::os::unix::fs::symlink(&target, &temp).unwrap();
        let err = stage_binary(&temp, b"new binary").unwrap_err().to_string();
        assert!(err.contains(&temp.display().to_string()), "{err}");
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[cfg(unix)]
        #[test]
        fn any_symlink_chain_resolves_to_the_real_file_and_is_stable(links in 0_usize..=6) {
            let dir = tempfile::TempDir::new().unwrap();
            let real = dir.path().join("llmenv-real");
            std::fs::write(&real, b"binary").unwrap();
            let mut target = real.clone();
            for i in 0..links {
                let link = dir.path().join(format!("link-{i}"));
                std::os::unix::fs::symlink(&target, &link).unwrap();
                target = link;
            }
            let resolved = resolve_install_path(&target).unwrap();
            prop_assert_eq!(&resolved, &std::fs::canonicalize(&real).unwrap());
            prop_assert_eq!(resolve_install_path(&resolved).unwrap(), resolved.clone());
        }
    }
}
