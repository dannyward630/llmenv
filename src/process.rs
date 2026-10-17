//! Child processes with a deadline.

use std::io::Read;
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

/// How often the deadline is checked. A short wait keeps a fast command fast.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Run `cmd` and wait at most `timeout`. The child is killed on a timeout, and the error kind is
/// [`std::io::ErrorKind::TimedOut`].
///
/// The piped stdout and stderr are read on their own threads while the child runs. A child that
/// writes more than the pipe holds would otherwise block on the write and never exit (#2832).
///
/// # Errors
/// The command cannot start, its output cannot be read, or it runs past `timeout`.
pub(crate) fn run_with_timeout(mut cmd: Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = cmd.spawn()?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            kill(&mut child);
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("no answer within {} ms", timeout.as_millis()),
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    Ok(Output {
        status,
        stdout: join(stdout)?,
        stderr: join(stderr)?,
    })
}

type Drain = Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>;

fn drain(pipe: Option<impl Read + Send + 'static>) -> Drain {
    pipe.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes)?;
            Ok(bytes)
        })
    })
}

fn join(handle: Drain) -> std::io::Result<Vec<u8>> {
    match handle {
        None => Ok(Vec::new()),
        Some(handle) => handle
            .join()
            .map_err(|_| std::io::Error::other("the thread that read the child output panicked"))?,
    }
}

fn kill(child: &mut Child) {
    if let Err(e) = child.kill() {
        tracing::error!("cannot kill child process {}: {e}", child.id());
    }
    if let Err(e) = child.wait() {
        tracing::error!("cannot reap child process {}: {e}", child.id());
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use super::run_with_timeout;

    #[test]
    fn a_command_that_runs_too_long_is_killed() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30").stdout(Stdio::piped()).stderr(Stdio::piped());
        let err = run_with_timeout(cmd, Duration::from_millis(100)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        let mut quick = Command::new("echo");
        quick
            .arg("hi")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = run_with_timeout(quick, Duration::from_secs(5)).unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
    }

    #[test]
    fn output_larger_than_a_pipe_does_not_block_the_child() {
        // 1 MiB is far past the 64 KiB pipe buffer, so an undrained pipe would hang the child.
        let mut cmd = Command::new("head");
        cmd.args(["-c", "1048576", "/dev/zero"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = run_with_timeout(cmd, Duration::from_secs(10)).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), 1_048_576);
    }
}
