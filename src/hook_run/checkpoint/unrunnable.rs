//! Checkpoints that can never run again, because the working directory of the first run is gone
//! (#2912).

use std::path::Path;

use super::{Checkpoint, complete};

/// Why `checkpoint` can never run again, or `None` when it can. A job runs in the working directory
/// of its first run, so a job whose directory is gone fails on every start and is never pruned,
/// because each failed start restores its file (#2912). A directory that cannot be read for another
/// reason is kept, because the cause can be temporary.
pub(super) fn unrunnable_reason(checkpoint: &Checkpoint) -> Option<String> {
    let Some(cwd) = checkpoint.cwd.as_deref() else {
        return Some("it has no recorded working directory".to_string());
    };
    match std::fs::metadata(cwd) {
        Ok(meta) if meta.is_dir() => None,
        Ok(_) => Some(format!("its working directory {cwd} is not a directory")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Some(format!("its working directory {cwd} is gone"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => Some(format!(
            "a parent of its working directory {cwd} is not a directory"
        )),
        Err(e) => {
            tracing::error!("cannot check the working directory {cwd} of a background job: {e}");
            None
        }
    }
}

/// Delete a checkpoint that can never run, and tell the agent once which job it was.
pub(super) fn drop_unrunnable(path: &Path, checkpoint: &Checkpoint, reason: &str) {
    let job = path
        .file_name()
        .map_or_else(|| checkpoint.kind.as_str().into(), |n| n.to_string_lossy());
    match complete(path) {
        Ok(()) => crate::agent_notice::record_logged(format!(
            "llmenv: removed the unfinished background job {job}, because {reason}. \
             The job cannot run again."
        )),
        Err(e) => crate::agent_notice::record_logged(format!(
            "llmenv: the background job {job} cannot run again, because {reason}, but its \
             checkpoint could not be removed ({e:#}). Fix: run `llmenv doctor` to check the \
             state directory."
        )),
    }
}
