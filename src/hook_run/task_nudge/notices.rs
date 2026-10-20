//! The agent notices of the task nudges, for a nudge state that has no file or was not saved.

use std::path::Path;

use super::MUTATING_TOOLS;

/// Whether a call of `tool` needs the nudge state: a skill start and a file edit count toward a
/// reminder. A read-only call has no nudge to lose, so a missing state is not worth a notice.
pub(super) fn needs_state(tool: &str) -> bool {
    tool == "Skill" || MUTATING_TOOLS.contains(&tool)
}

pub(super) fn note_no_state(session_id: Option<&str>) {
    let cause = match session_id {
        None => "the hook payload has no session id".to_owned(),
        Some(id) => format!("the session id {id:?} is not a plain name"),
    };
    crate::agent_notice::record_logged(format!(
        "llmenv skipped the task nudge check: {cause}, so the nudge state has no file. Fix: run \
         `llmenv doctor` to check the engine's hook payload."
    ));
}

/// Logs and notices a nudge state change that was not saved, because the lock was busy or
/// could not be taken.
pub(super) fn note_unlocked_skip(path: &Path) {
    crate::agent_notice::record_logged(format!(
        "llmenv: the task nudge state {} was not saved, because its lock is busy or cannot be \
         taken. A nudge may repeat. Fix: run `llmenv doctor` to check the state directory.",
        path.display()
    ));
}
