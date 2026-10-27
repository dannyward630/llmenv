//! MCP handshake and tool-call trace telemetry.
//!
//! These functions emit `[LLMENV_MCP_CALL]` lines when
//! `LLMENV_TRACE_TIMING` is set. They are separated from `trace.rs` so that
//! `trace.rs` stays near its line cap.

use std::time::Duration;

use std::sync::atomic::{AtomicU64, Ordering};

use crate::trace::emit_trace_line;

/// Tool calls this process has made. A detached job reports it on exit (#3014).
static MCP_CALLS: AtomicU64 = AtomicU64::new(0);

/// Count one MCP tool call made by this process.
pub(crate) fn count_mcp_call() {
    MCP_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// Return how many MCP tool calls this process has made.
pub(crate) fn mcp_call_count() -> u64 {
    MCP_CALLS.load(Ordering::Relaxed)
}

/// Emit a `[LLMENV_MCP_CALL] initialize <us>us` or
/// `[LLMENV_MCP_CALL] notifications/initialized <us>us` line to the trace
/// sink. The leading `<tool> <us>us` fields are kept because consumers
/// already parse them.
pub(crate) fn emit_mcp_handshake_trace(
    method: &str,
    elapsed: Duration,
    status: Option<u16>,
    req_bytes: u64,
    resp_bytes: u64,
) {
    if !crate::trace::enabled() {
        return;
    }
    emit_trace_line(&crate::trace::lines::mcp_call_line(
        method, elapsed, req_bytes, resp_bytes, status,
    ));
}

/// Emit a `[LLMENV_MCP_CALL] <tool> <duration>us req=<bytes> resp=<bytes>`
/// line to the trace sink.
pub(crate) fn emit_mcp_call_trace(tool: &str, elapsed: Duration, req_bytes: u64, resp_bytes: u64) {
    if !crate::trace::enabled() {
        return;
    }
    emit_trace_line(&crate::trace::lines::mcp_call_line(
        tool, elapsed, req_bytes, resp_bytes, None,
    ));
}
