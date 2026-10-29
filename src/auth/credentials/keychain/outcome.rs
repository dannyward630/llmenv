//! The exit status of `security delete-generic-password`, read as a result.

use super::SECURITY_ITEM_NOT_FOUND_EXIT_CODE;

/// Read the exit status of `security delete-generic-password`. Exit 44 (errSecItemNotFound) is
/// the only "nothing to delete" answer. Any other failure is an error, so a locked keychain does
/// not look like a clean run (#3068).
pub(super) fn forget_outcome(
    service: &str,
    status: &std::process::ExitStatus,
    stderr: &str,
) -> anyhow::Result<bool> {
    if status.success() {
        return Ok(true);
    }
    if status.code() == Some(SECURITY_ITEM_NOT_FOUND_EXIT_CODE) {
        return Ok(false);
    }
    // The same scrub as git failure text, so control bytes never reach the terminal.
    let detail = if stderr.trim().is_empty() {
        String::new()
    } else {
        format!(
            ": {}",
            llmenv_git::git_failure_detail(stderr.as_bytes(), &[], *status)
        )
    };
    Err(anyhow::anyhow!(
        "deleting keychain item {service} failed (status {status}){detail}. Fix: unlock the login \
         keychain (`security unlock-keychain`) and run the command again."
    ))
}
