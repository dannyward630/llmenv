//! Signals to a probe's process group.

/// Send `SIGTERM` to the process group that `pid` leads. The probe starts the server with
/// `process_group(0)`, so `pid` is the group's own id. A direct syscall: no `kill` process starts.
pub(super) fn signal_group(pid: u32) -> std::io::Result<()> {
    let pid = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| std::io::Error::other(format!("pid {pid} is not a process")))?;
    rustix::process::kill_process_group(pid, rustix::process::Signal::TERM)
        .map_err(std::io::Error::from)
}
