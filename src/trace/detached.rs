//! Detached job trace emitter and CPU-time probe (#3014).

use crate::trace::emit_trace_line;

/// One detached job that is about to exit.
pub(crate) struct DetachedJob<'a> {
    /// Fixed tag such as `consolidation` or `icm-store`.
    pub kind: &'a str,
    /// The request id the parent logged for this job, when the job has one.
    pub request_id: Option<&'a str>,
    /// When the job started.
    pub started: std::time::Instant,
    /// The job exit code: 0 for success, 1 for a failure.
    pub exit: i32,
}

/// Emit `[LLMENV_DETACHED] <kind> <wall_us>us cpu_us=<n> mcp_calls=<n> exit=<code>[ request_id=<id>]`.
///
/// The line goes to the trace sink. A detached job has no visible stderr, so without
/// `LLMENV_TRACE_FILE` the line is also appended to `detached-trace.log` in the state dir.
pub(crate) fn emit_detached_trace(job: &DetachedJob<'_>) {
    if !crate::trace::enabled() {
        return;
    }
    let wall_us = u64::try_from(job.started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let cpu_us = cpu_time_us();
    let mcp_calls = crate::mcp_trace::mcp_call_count();
    let (kind, exit) = (job.kind, job.exit);
    let mut line = format!(
        "[LLMENV_DETACHED] {kind} {wall_us}us cpu_us={cpu_us} mcp_calls={mcp_calls} exit={exit}"
    );
    if let Some(id) = job.request_id.filter(|id| !id.is_empty()) {
        line.push_str(&format!(" request_id={}", crate::util::escape_control(id)));
    }
    emit_trace_line(&line);
    if std::env::var_os("LLMENV_TRACE_FILE").is_none() {
        match llmenv_paths::state_dir() {
            Ok(dir) => crate::trace::append_trace_line(&dir.join("detached-trace.log"), &line),
            Err(e) => tracing::warn!("detached trace: no state dir, line dropped: {e:#}"),
        }
    }
}

/// Return user+sys CPU time of this process as microseconds.
pub(crate) fn cpu_time_us() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    let secs = u64::try_from(t.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(t.tv_nsec).unwrap_or(0);
    secs * 1_000_000 + nanos / 1_000
}

/// Read the string field `key` from a JSON payload, or `None` when the payload or field is absent.
pub(crate) fn json_str(payload_json: &str, key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload_json).ok()?;
    value.get(key)?.as_str().map(str::to_string)
}
