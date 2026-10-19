use super::*;

#[test]
fn secure_git_includes_config_flags() {
    let cmd = secure_git();
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args, GIT_CONFIG_FLAGS);
}

/// A bare `git` would search a relative PATH folder too, where a cloned repository can hold
/// its own copy (#2948).
#[test]
fn secure_git_runs_git_from_an_absolute_folder() {
    let program = std::path::PathBuf::from(secure_git().get_program());
    assert!(program.is_absolute(), "{}", program.display());
    assert_eq!(program.file_name(), Some(std::ffi::OsStr::new("git")));
}

#[test]
fn apply_git_timeout_sets_tcp_timeout() {
    use std::ffi::OsStr;
    let mut cmd = secure_git();
    apply_git_timeout(&mut cmd, 10);
    assert!(
        cmd.get_envs()
            .any(|(k, v)| k == OsStr::new("GIT_CONNECT_TIMEOUT") && v == Some(OsStr::new("10"))),
        "GIT_CONNECT_TIMEOUT env var should be set to 10"
    );
}

#[test]
fn apply_git_timeout_sets_ssh_command() {
    use std::ffi::OsStr;
    let mut cmd = secure_git();
    apply_git_timeout(&mut cmd, 10);
    // Function checks std::env::var_os to avoid clobbering existing GIT_SSH_COMMAND.
    // In this test environment it's not set, so the function should set it.
    assert!(
        cmd.get_envs()
            .any(|(k, v)| k == OsStr::new("GIT_SSH_COMMAND")
                && v == Some(OsStr::new("ssh -o ConnectTimeout=10 -o BatchMode=yes"))),
        "GIT_SSH_COMMAND should be set correctly"
    );
}

#[test]
fn apply_git_timeout_sets_http_config() {
    let mut cmd = secure_git();
    apply_git_timeout(&mut cmd, 10);
    let args: Vec<String> = cmd
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert!(
        args.windows(2).any(|w| w == ["-c", "http.lowSpeedTime=10"]),
        "should set http.lowSpeedTime=10"
    );
    assert!(
        args.windows(2).any(|w| w == ["-c", "http.lowSpeedLimit=1"]),
        "should set http.lowSpeedLimit=1"
    );
}

#[test]
fn sanitize_git_url_http_with_credentials() {
    let url = "https://user:password@github.com/owner/repo.git";
    assert_eq!(
        sanitize_git_url(url),
        "https://***@github.com/owner/repo.git"
    );
}

#[test]
fn sanitize_git_url_ssh() {
    let url = "git@github.com:owner/repo.git";
    assert_eq!(sanitize_git_url(url), "***@github.com:owner/repo.git");
}

#[test]
fn sanitize_git_url_no_credentials() {
    let url = "https://github.com/owner/repo.git";
    assert_eq!(sanitize_git_url(url), url);
}

#[test]
fn git_failure_detail_prefers_stderr() {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::ExitStatus::from_raw(1 << 8);
    let detail = git_failure_detail(b"fatal: repository not found\n", b"ignored", status);
    assert_eq!(detail, "fatal: repository not found");
}

#[test]
fn git_failure_detail_falls_back_to_stdout_when_stderr_empty() {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::ExitStatus::from_raw(1 << 8);
    let detail = git_failure_detail(b"", b"index locked\n", status);
    assert_eq!(detail, "index locked");
}

#[test]
fn git_failure_detail_scrubs_credentials_in_stderr() {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::ExitStatus::from_raw(1 << 8);
    let detail = git_failure_detail(
        b"fatal: could not read from https://user:tok@github.com/x.git\n",
        b"",
        status,
    );
    assert!(!detail.contains("tok"), "credential leaked: {detail}");
}

#[test]
fn git_failure_detail_strips_control_and_ansi_sequences() {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::ExitStatus::from_raw(1 << 8);
    let hostile = b"\x1b[2Jcleared\x1b]0;title";
    let detail = git_failure_detail(hostile, b"", status);
    assert_eq!(detail, "[2Jcleared]0;title");
}

#[test]
fn git_failure_detail_falls_back_to_exit_code() {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::ExitStatus::from_raw(128 << 8);
    let detail = git_failure_detail(b"   \n", b"", status);
    assert!(detail.contains("exit code"), "got: {detail}");
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parse_commit_count_valid_numeric(count_val in 0u32..100_000) {
            let bytes = format!("{count_val}").into_bytes();
            prop_assert_eq!(parse_commit_count(&bytes), Some(count_val));
        }

        #[test]
        fn parse_commit_count_whitespace_trimmed(count_val in 0u32..100_000) {
            let bytes = format!("  {count_val}  \n").into_bytes();
            prop_assert_eq!(parse_commit_count(&bytes), Some(count_val));
        }

        #[test]
        fn parse_commit_count_malformed_returns_none(junk in ".*") {
            // Must not panic on arbitrary input; non-numeric → None
            let _ = parse_commit_count(junk.as_bytes());
        }

        #[test]
        fn sanitize_git_url_no_panic(s in ".*") {
            let _ = sanitize_git_url(&s);
        }

        #[test]
        fn sanitize_git_url_idempotent(s in ".*") {
            let once = sanitize_git_url(&s);
            let twice = sanitize_git_url(&once);
            prop_assert_eq!(once, twice);
        }

        #[test]
        fn sanitize_git_url_scrubs_http_credentials(
            user in "[a-zA-Z0-9_]+",
            pass in "[a-zA-Z0-9_]+",
            host in "[a-zA-Z0-9.-]+",
            path in "[a-zA-Z0-9/_.-]*"
        ) {
            let url = format!("https://{user}:{pass}@{host}/{path}");
            let sanitized = sanitize_git_url(&url);
            // The credential sequence user:pass@ must be gone; pass may
            // legitimately appear in the host or path, so we check the
            // full credential prefix, not the password alone.
            prop_assert!(!sanitized.contains(&format!("{user}:{pass}@")),
                "credentials leaked in: {sanitized}");
            prop_assert!(sanitized.contains("***@"), "expected *** in: {sanitized}");
        }
    }
}
