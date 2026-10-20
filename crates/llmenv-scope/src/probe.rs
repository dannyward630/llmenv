//! Scope-detection commands, each run with a deadline.

use std::time::Duration;

/// How long one scope-detection command may run. The detectors run on every shell prompt, where
/// a hang blocks the prompt, and a healthy `route`, `arp`, or `networksetup` call answers far
/// sooner. It is fixed, not configurable: a slower answer means the command is wedged.
pub(crate) const DEFAULT_DETECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Run a scope-detection command for at most `timeout` and capture its stdout.
///
/// Returns `None` when `program` is not in an absolute `PATH` folder, and on spawn error,
/// timeout, non-zero exit, or non-UTF-8 output. These
/// detectors run on every shell prompt, so a failure is intentionally silent at
/// the return value (→ the scope simply doesn't match); the `tracing::debug!`
/// keeps the cause recoverable under `RUST_LOG=debug` without spamming normal
/// runs. stdin is null and stderr is captured, so this never blocks on a prompt
/// nor leaks to the terminal (#307). The deadline stops a wedged command from
/// blocking the prompt (#2885).
pub(crate) fn capture_stdout(
    label: &str,
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Option<String> {
    // A spawn by bare name searches relative PATH folders too, and the working folder can be a
    // cloned repository that holds a planted copy (#2948).
    let path = match llmenv_paths::program::resolve_program(program) {
        Ok(path) => path,
        Err(e) => {
            tracing::debug!("{label}: {e:#}");
            return None;
        }
    };
    let mut cmd = std::process::Command::new(path);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = match llmenv_util::process::run_with_timeout(cmd, timeout) {
        Ok(out) => out,
        Err(e) => {
            tracing::debug!("{label}: running {program} failed: {e}");
            return None;
        }
    };
    if !out.status.success() {
        tracing::debug!(
            "{label}: {program} exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
        return None;
    }
    match String::from_utf8(out.stdout) {
        Ok(text) => Some(text),
        Err(e) => {
            tracing::debug!("{label}: {program} printed non-UTF-8 output: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn capture_stdout_gives_up_on_a_command_past_its_deadline() {
        let started = std::time::Instant::now();
        let out = capture_stdout("test", "/bin/sleep", &["600"], Duration::from_millis(100));
        assert_eq!(out, None);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the run must stop at the deadline"
        );
    }

    #[cfg(unix)]
    #[test]
    fn capture_stdout_returns_the_output_of_a_command_that_answers() {
        let out = capture_stdout("test", "/bin/echo", &["hi"], DEFAULT_DETECT_TIMEOUT);
        assert_eq!(out.as_deref(), Some("hi\n"));
    }

    /// A bare name runs the copy in an absolute PATH folder. `find_on_path` skips relative
    /// folders, as its own tests show (#2948).
    #[cfg(unix)]
    #[test]
    fn capture_stdout_finds_a_bare_name_on_the_absolute_path() {
        let out = capture_stdout("test", "echo", &["hi"], DEFAULT_DETECT_TIMEOUT);
        assert_eq!(out.as_deref(), Some("hi\n"));
        let missing = capture_stdout(
            "test",
            "llmenv-no-such-program-2948",
            &[],
            DEFAULT_DETECT_TIMEOUT,
        );
        assert_eq!(missing, None);
    }
}
