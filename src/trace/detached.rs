//! Detached job trace emitter and CPU-time probe (#3014).

use crate::trace::emit_trace_line;

/// Emit a `[LLMENV_DETACHED] <kind> <wall_us>us cpu_us=<n> mcp_calls=<n> exit=<code|error>` line to
/// the trace sink.
///
/// `kind` is a fixed string such as `consolidation` or `icm-store`. `mcp_calls`
/// is the number of MCP tool calls the job made (zero when unknown). `exit`
/// is the job exit code, or `error` when the job did not complete.
pub(crate) fn emit_detached_trace(
    kind: &str,
    wall_us: u64,
    cpu_us: u64,
    mcp_calls: u64,
    exit: Option<i32>,
) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    let exit_str = match exit {
        Some(code) => format!("exit={code}"),
        None => "exit=error".to_string(),
    };
    emit_trace_line(&format!(
        "[LLMENV_DETACHED] {kind} {wall_us}us cpu_us={cpu_us} mcp_calls={mcp_calls} {exit_str}"
    ));
}

/// Return user+sys CPU time of this process as microseconds.
pub(crate) fn cpu_time_us() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    let secs = u64::try_from(t.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(t.tv_nsec).unwrap_or(0);
    secs * 1_000_000 + nanos / 1_000
}
