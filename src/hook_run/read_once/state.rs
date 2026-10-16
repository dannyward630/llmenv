//! Where the read-once cache lives, and how it is cleared.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Build the state subdirectory path for read-once cache files.
pub fn read_once_state_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("read_once")
}

/// Build the full path to a session's cache file.
pub(in crate::hook_run) fn session_cache_path(state_dir: &Path, session_id: &str) -> PathBuf {
    read_once_state_dir(state_dir).join(format!("{session_id}.json"))
}

/// Clear the entire read-once cache by removing the state directory.
pub fn clear_cache() -> anyhow::Result<()> {
    let state_dir = crate::paths::state_dir()?;
    let ro_dir = read_once_state_dir(&state_dir);
    if ro_dir.exists() {
        std::fs::remove_dir_all(&ro_dir)?;
        writeln!(std::io::stdout(), "Read-once cache cleared")?;
    } else {
        writeln!(std::io::stdout(), "No read-once cache to clear")?;
    }
    Ok(())
}

/// The cache key of `path`: its canonical path. A path that cannot be resolved is keyed by its
/// raw form, and the agent is told (#2818).
pub(super) fn cache_key(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path)
        .inspect_err(|e| {
            // Hook stderr never reaches the agent, so the notice goes to the agent (#2818).
            crate::agent_notice::record(format!(
                "llmenv: read-once could not resolve {}: {e}. Repeat reads use the raw path as the \
             key. Fix: check the file path and its parent directory.",
                path.display()
            ));
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "cannot canonicalize read-once cache path, using raw path"
            )
        })
        .unwrap_or_else(|_| path.to_path_buf());
    canonical.to_string_lossy().to_string()
}
