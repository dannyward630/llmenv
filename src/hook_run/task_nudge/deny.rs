//! The deny texts of the task commit gate. The gate denies a commit that it cannot check, rather
//! than let it pass (#2630).

use std::path::Path;

use crate::config::TaskTracker;
use crate::hook_run::shell_split::runs_commit_or_pr;

/// The `__DENY__` text for a commit when the gate cannot check it. The gate denies rather than
/// passing a commit it could not check (#2630).
pub(super) fn gate_unavailable_deny(reason: &str) -> String {
    format!(
        "__DENY__:llmenv blocked this commit or pull request: the task commit gate cannot check \
         it, because {reason}. Fix that, then run the same command again. To turn this off, set \
         features.task_tracker.enforce_commit to false."
    )
}

/// The `__DENY__` text for a commit when the deny marker cannot be saved to `path`.
pub(super) fn unsaved_state_deny(path: &Path) -> String {
    gate_unavailable_deny(&format!(
        "the task nudge state {} cannot be saved",
        path.display()
    ))
}

/// The deny for a commit when the llmenv state directory cannot be resolved, for the dispatcher.
/// The commit gate reads its state there, so the commit is denied (#2630). `None` for any other
/// call.
pub(crate) fn state_dir_deny(
    tracker: &TaskTracker,
    payload: &serde_json::Value,
    error: &anyhow::Error,
) -> Option<String> {
    let command = payload["tool_input"]["command"].as_str()?;
    if !tracker.enforce_commit
        || payload["tool_name"].as_str() != Some("Bash")
        || !runs_commit_or_pr(command)
    {
        return None;
    }
    Some(gate_unavailable_deny(&format!(
        "the llmenv state directory cannot be resolved ({error:#})"
    )))
}

/// The `__DENY__` text for a commit when the nudge state file cannot be read. The gate cannot
/// tell whether the commit has a task, so it denies every attempt until the file is readable (#2630).
pub(super) fn state_unreadable_deny(error: &str) -> String {
    format!(
        "__DENY__:llmenv blocked this commit or pull request: the task nudge state cannot be read \
         ({error}). Fix the file or its permissions, then run the same command again. To turn this \
         off, set features.task_tracker.enforce_commit to false."
    )
}

/// The `__DENY__` text for a commit when the task store cannot be read. The gate cannot tell
/// whether a task is in progress, so it denies (#2630).
pub(super) fn store_unreadable_deny() -> String {
    "__DENY__:llmenv blocked this commit or pull request: the task store cannot be read, so the \
     gate cannot tell whether a task is in progress. Fix the task store under the state directory, \
     then run the same command again. To turn this off, set features.task_tracker.enforce_commit \
     to false."
        .to_owned()
}
