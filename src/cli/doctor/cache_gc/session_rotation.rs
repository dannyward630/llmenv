//! Removes old session-log rotations from the state folder (#2816).

use std::path::Path;
use std::time::Duration;

use crate::cli::doctor::cache_gc::GcOutput;
use crate::session_log;

/// Remove session-log rotations older than the retention (#2816). The active file is kept.
/// Returns 1 when the pass failed.
pub(super) fn prune_session_log(state_dir: &Path, retention: Duration, out: GcOutput<'_>) -> usize {
    let path = session_log::file_sink::file_path_in(state_dir);
    match session_log::rotation::prune_rotations(&path, retention, out.dry_run) {
        Ok(removed) => {
            eprintln!(
                "{} GC: {} {} rotated session-log file(s)",
                out.pass,
                out.verb(),
                removed.len()
            );
            out.list(&removed);
            0
        }
        Err(e) => {
            eprintln!("{} GC of session-log rotations failed: {e:#}", out.warn);
            1
        }
    }
}
