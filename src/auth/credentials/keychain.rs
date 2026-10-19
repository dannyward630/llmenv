//! The macOS keychain store of the credential, through `/usr/bin/security` and the Security
//! framework.

use std::path::Path;
#[cfg(any(target_os = "macos", test))]
use std::time::Duration;

/// The longest `security delete-generic-password` may run. It shows no prompt for an item that
/// llmenv created, so a stall is a locked keychain or a stuck `securityd` (#2892).
#[cfg(target_os = "macos")]
const FORGET_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest `security find-generic-password` may run. It can show the macOS keychain access
/// prompt, and the user needs time to answer it (#2892).
#[cfg(target_os = "macos")]
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Run one `security` call with a deadline. A timeout names the operation, the keychain item, and
/// the fix, because a locked keychain blocks the call with no output.
#[cfg(any(target_os = "macos", test))]
fn run_security(
    cmd: std::process::Command,
    timeout: Duration,
    operation: &str,
    service: &str,
) -> anyhow::Result<std::process::Output> {
    crate::process::run_with_timeout(cmd, timeout).map_err(|e| {
        if e.kind() == std::io::ErrorKind::TimedOut {
            anyhow::anyhow!(
                "`security {operation}` for keychain item {service} gave no answer within {} s. \
                 Fix: unlock the login keychain (`security unlock-keychain`), answer any macOS \
                 keychain prompt, then run the command again.",
                timeout.as_secs()
            )
        } else {
            anyhow::anyhow!("running `security {operation}` for keychain item {service}: {e}")
        }
    })
}

/// Drop the keychain credential belonging to a config dir that no longer exists.
///
/// Returns `true` when an item was deleted. No-op off macOS, where the
/// credential lives inside the folder and dies with it.
///
/// # Errors
/// Returns an error when `security` cannot be run, or gives no answer within 10 s.
#[cfg(target_os = "macos")]
pub(crate) fn forget(config_dir: &Path) -> anyhow::Result<bool> {
    let service = super::keychain_service(config_dir);
    let account = keychain_account()?;
    // stderr discarded deliberately (#1092): the overwhelmingly common failure
    // is "item not found", which the `bool` return already communicates as
    // `false` — there's nothing more a caller needs from stderr for a delete.
    let mut cmd = std::process::Command::new(SECURITY_BIN);
    cmd.args(["delete-generic-password", "-s", &service, "-a", &account])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let out = run_security(cmd, FORGET_TIMEOUT, "delete-generic-password", &service)?;
    Ok(out.status.success())
}

/// Drop the keychain credential belonging to a config dir that no longer exists.
///
/// Returns `true` when an item was deleted. No-op off macOS, where the
/// credential lives inside the folder and dies with it.
///
/// # Errors
/// Never fails on this platform.
#[cfg(not(target_os = "macos"))]
pub(crate) fn forget(_config_dir: &Path) -> anyhow::Result<bool> {
    Ok(false)
}

/// Absolute path so a hijacked `PATH` cannot substitute the keychain tool.
#[cfg(target_os = "macos")]
const SECURITY_BIN: &str = "/usr/bin/security";

#[cfg(target_os = "macos")]
fn keychain_account() -> anyhow::Result<String> {
    crate::env_var::utf8_var("USER")?.ok_or_else(|| {
        anyhow::anyhow!("USER is unset; cannot address the keychain credential item")
    })
}

/// `security find-generic-password`'s exit code for "no matching item"
/// (errSecItemNotFound, OSStatus -25300) — verified empirically, since
/// `security` does not document its exit codes. Classifying on this rather
/// than on stderr text matters (#1092): `security` localizes its error
/// strings via `SecCopyErrorMessageString`, so on a non-English system a
/// string match would silently miss every failure and collapse back to the
/// same "absent" outcome the classification exists to avoid.
#[cfg(target_os = "macos")]
const SECURITY_ITEM_NOT_FOUND_EXIT_CODE: i32 = 44;

/// Why `security find-generic-password` failed — distinguishing these matters
/// because "genuinely absent" and "everything else" call for different
/// user-facing responses (#1092): a real problem (locked keychain,
/// permission error, …) should surface as an error, not collapse into the
/// same "no credential stored" outcome as a missing item.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeychainReadFailure {
    /// No matching item exists — the common, expected case
    /// ([`SECURITY_ITEM_NOT_FOUND_EXIT_CODE`]).
    Absent,
    /// Any other failure: locked keychain, permission error, I/O error, etc.
    /// Treated as a real problem rather than "no credential stored".
    Other,
}

/// Classify a failed `security find-generic-password` invocation from its
/// exit status. Pure function so it's testable without a real keychain.
#[cfg(target_os = "macos")]
fn classify_keychain_read_failure(status: &std::process::ExitStatus) -> KeychainReadFailure {
    if status.code() == Some(SECURITY_ITEM_NOT_FOUND_EXIT_CODE) {
        KeychainReadFailure::Absent
    } else {
        KeychainReadFailure::Other
    }
}

#[cfg(target_os = "macos")]
pub(super) fn keychain_read(config_dir: &Path) -> anyhow::Result<Option<super::Credentials>> {
    let service = super::keychain_service(config_dir);
    let account = keychain_account()?;
    let mut cmd = std::process::Command::new(SECURITY_BIN);
    cmd.args([
        "find-generic-password",
        "-w",
        "-s",
        &service,
        "-a",
        &account,
    ])
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
    // The call can wait on a macOS access prompt, so a person at the terminal is told what it
    // waits for (#2892). A hook has no reader for the line, so it is not printed there.
    if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        eprintln!(
            "llmenv: reading keychain item {service}. If macOS asks for keychain access, answer \
             the prompt."
        );
    }
    let out = run_security(cmd, READ_TIMEOUT, "find-generic-password", &service)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return match classify_keychain_read_failure(&out.status) {
            KeychainReadFailure::Absent => {
                tracing::debug!("keychain item {service} not found: {}", stderr.trim());
                Ok(None)
            }
            KeychainReadFailure::Other => Err(anyhow::anyhow!(
                "keychain lookup failed for service {service} (status {}): {}. If the \
                 keychain is locked, unlock it (e.g. `security unlock-keychain`) and retry.",
                out.status,
                stderr.trim()
            )),
        };
    }
    // Deliberately not logged or surfaced — stdout is the token itself.
    let raw = String::from_utf8_lossy(&out.stdout);
    match serde_json::from_str::<serde_json::Value>(raw.trim()) {
        Ok(value) => Ok(super::Credentials::from_json(value)),
        Err(e) => {
            tracing::debug!(
                "keychain item {service} is not a JSON credential (line {}, column {})",
                e.line(),
                e.column()
            );
            Ok(None)
        }
    }
}

/// Store a credential in the keychain via the Security framework directly
/// (#1061) — never through the `security` CLI's argv.
///
/// `security_framework::passwords::set_generic_password` wraps
/// `SecItemAdd`/`SecItemUpdate` (falling back to update on `errSecDuplicateItem`,
/// the same "create or update in place" semantics the old `-U` flag gave), with
/// the secret passed as an in-memory buffer. That closes both problems the CLI
/// approach had: the blob no longer sits in a child's argv (readable via `ps` by
/// other processes of the same user for the child's lifetime), and there's no
/// length limit to trip — the CLI's stdin form truncates at exactly 128 bytes
/// (measured), far short of a real ~510-byte credential blob, so it would have
/// silently stored a corrupt token with no error.
#[cfg(target_os = "macos")]
pub(super) fn keychain_write(config_dir: &Path, creds: &super::Credentials) -> anyhow::Result<()> {
    let service = super::keychain_service(config_dir);
    let account = keychain_account()?;
    let blob = creds.to_blob()?;
    security_framework::passwords::set_generic_password(&service, &account, blob.as_bytes())
        .map_err(|e| anyhow::anyhow!("writing keychain item for service {service}: {e}"))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_security_call_past_its_deadline_names_the_item_and_the_fix() {
        let mut cmd = std::process::Command::new("/bin/sleep");
        cmd.arg("5");
        let err = run_security(
            cmd,
            Duration::from_millis(50),
            "find-generic-password",
            "svc-x",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("find-generic-password") && err.contains("svc-x"),
            "{err}"
        );
        assert!(err.contains("Fix: unlock the login keychain"), "{err}");
    }

    #[test]
    fn a_security_call_that_cannot_start_names_the_operation() {
        let cmd = std::process::Command::new("/nonexistent/security");
        let err = run_security(
            cmd,
            Duration::from_secs(1),
            "delete-generic-password",
            "svc-y",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("delete-generic-password") && err.contains("svc-y"),
            "{err}"
        );
    }

    // -- keychain_read failure classification (#1092) --

    #[cfg(target_os = "macos")]
    fn exit_status(code: i32) -> std::process::ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn classifies_item_not_found_exit_code_as_absent() {
        assert_eq!(
            classify_keychain_read_failure(&exit_status(SECURITY_ITEM_NOT_FOUND_EXIT_CODE)),
            KeychainReadFailure::Absent
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn classifies_any_other_exit_code_as_other() {
        for code in [1, 36, 51, 2] {
            assert_eq!(
                classify_keychain_read_failure(&exit_status(code)),
                KeychainReadFailure::Other,
                "exit code {code} should classify as Other"
            );
        }
    }
}
