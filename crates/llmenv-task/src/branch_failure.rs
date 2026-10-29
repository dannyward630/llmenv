//! Classifies a failed `git symbolic-ref` run for the task tracker (#3040).

/// Whether a failed `git symbolic-ref --quiet --short HEAD` is worth telling the user about.
/// Exit 1 is a detached HEAD, and "not a git repository" is a directory outside git. Both are
/// normal. Anything else, such as "dubious ownership" or a kill by signal, hides the branch
/// for a reason the user can fix.
pub(super) fn branch_failure_is_unexpected(code: Option<i32>, stderr: &str) -> bool {
    code != Some(1) && !llmenv_git::is_not_a_repo_message(stderr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_failure_is_unexpected_ignores_a_detached_head_and_a_directory_outside_git() {
        assert!(!branch_failure_is_unexpected(Some(1), ""));
        assert!(!branch_failure_is_unexpected(
            Some(128),
            "fatal: not a git repository (or any of the parent directories): .git"
        ));
    }

    #[test]
    fn branch_failure_is_unexpected_ignores_a_capitalised_not_a_git_repository() {
        // git may capitalise its message, so the match ignores case (#3040).
        assert!(!branch_failure_is_unexpected(
            Some(128),
            "Fatal: Not a git repository (or any of the parent directories): .git"
        ));
    }

    #[test]
    fn branch_failure_is_unexpected_flags_ownership_and_signal_failures() {
        assert!(branch_failure_is_unexpected(
            Some(128),
            "fatal: detected dubious ownership in repository at '/x'"
        ));
        assert!(branch_failure_is_unexpected(None, ""));
        assert!(branch_failure_is_unexpected(Some(2), "usage: git"));
    }
}
