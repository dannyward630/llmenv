//! The restart of a background job from its checkpoint, in the folder of its first run.

use std::io::Write as _;

use anyhow::Context as _;

use super::checkpoint;
use super::{
    consolidation_run_command, detached_child_log_path, icm_store_command,
    redirect_stderr_to_detached_log,
};

/// Start the child for one stale checkpoint, with the inputs and the working directory of the
/// first run. The memory backend and the project come from that directory, so a checkpoint whose
/// directory is gone is not run: it would store into the project of the session that resumes it.
pub(super) fn respawn_job(
    cp: &checkpoint::Checkpoint,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    respawn_job_with_exe(cp, path, std::env::current_exe)
}

/// [`respawn_job`] with the image lookup passed in, so a test can make it fail.
pub(super) fn respawn_job_with_exe(
    cp: &checkpoint::Checkpoint,
    path: &std::path::Path,
    current_exe: impl FnOnce() -> std::io::Result<std::path::PathBuf>,
) -> anyhow::Result<()> {
    use checkpoint::JobKind;
    let cwd = cp.cwd.as_deref().map(std::path::Path::new).ok_or_else(|| {
        anyhow::anyhow!("the working directory of the first run was not recorded")
    })?;
    // A stat error that is not "gone" can be temporary, so name the real cause.
    match std::fs::metadata(cwd) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => anyhow::bail!(
            "the working directory of the first run ({}) is not a folder",
            cwd.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => anyhow::bail!(
            "the working directory of the first run ({}) is gone",
            cwd.display()
        ),
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "cannot use the working directory of the first run ({})",
                    cwd.display()
                )
            });
        }
    }
    // Spawns this same binary as a detached child. The path is this process's own image, not input.
    // nosemgrep: rust.lang.security.current-exe.current-exe
    let exe = current_exe().context("cannot resolve current_exe")?;
    let stdin = cp.inputs.to_string();
    match cp.kind {
        JobKind::IcmStore => spawn_detached(icm_store_command(exe, Some(path)), Some(&stdin), cwd),
        JobKind::SessionLogRecord => spawn_detached(
            crate::session_log::detached::record_command(exe, Some(path)),
            Some(&stdin),
            cwd,
        ),
        JobKind::Consolidation => {
            spawn_detached(consolidation_run_command(exe, Some(path)), None, cwd)
        }
        // Not resumable: `resume_pending` skips it, because `SessionStart` starts the indexer.
        JobKind::CbmIndex => Ok(()),
    }
}

/// Spawn `cmd` as a detached child in `cwd` with the shared bounded stderr log, and write `stdin`
/// to it. Not waited on.
pub(super) fn spawn_detached(
    mut cmd: std::process::Command,
    stdin: Option<&str>,
    cwd: &std::path::Path,
) -> anyhow::Result<()> {
    cmd.current_dir(cwd);
    redirect_stderr_to_detached_log(&mut cmd, detached_child_log_path());
    crate::mcp::proxy::detach_process_group(&mut cmd);
    let mut child = cmd.spawn()?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())?;
    }
    Ok(())
}
