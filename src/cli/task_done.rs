//! `llmenv task done`: close a task and print the notes the close leaves behind.

use std::path::Path;

use super::{caller_session_for_task, guard_task, record_override};

/// Close task `id` and print what the close left behind: a skipped start, undone sub-tasks, or
/// the hint for the last open task in its session (#2987).
pub(super) fn run(
    state_dir: &Path,
    id: &str,
    force: bool,
    other_session: Option<&str>,
) -> anyhow::Result<()> {
    let audit = guard_task(state_dir, id, other_session)?;
    // The caller is resolved before the close, so a failure here leaves the task open.
    let caller = caller_session_for_task(state_dir)?;
    let completed = crate::task::complete_task_by(state_dir, id, force, &caller)?;
    println!("Completed '{}'", completed.task.slug);
    if let Some(note) = completed.skipped_start_note() {
        println!("{note}");
    }
    if let Some(note) = completed.undone_children_note() {
        println!("{note}");
    }
    if let Some(hint) = crate::task::last_task_hint(state_dir, &completed.task)? {
        println!("{hint}");
    }
    record_override(state_dir, audit)
}
