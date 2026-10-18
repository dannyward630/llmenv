//! Replace the running llmenv binary with a downloaded release.

use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::{Path, PathBuf};
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
    copy_refusing_symlink(&current_exe, &backup).with_context(|| {
        format!(
            "failed to back up {} to {}",
            current_exe.display(),
            backup.display()
        )
    })?;

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
