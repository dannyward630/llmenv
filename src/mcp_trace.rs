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
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    let us = elapsed.as_micros();
    let status_str = match status {
        Some(code) => format!(" status={code}"),
        None => String::new(),
    };
    emit_trace_line(&format!(
        "[LLMENV_MCP_CALL] {method} {us}us req={req_bytes} resp={resp_bytes}{status_str}"
    ));
}

/// Emit a `[LLMENV_MCP_CALL] <tool> <duration>us req=<bytes> resp=<bytes>`
/// line to the trace sink.
pub(crate) fn emit_mcp_call_trace(tool: &str, elapsed: Duration, req_bytes: u64, resp_bytes: u64) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    let us = elapsed.as_micros();
    emit_trace_line(&format!(
        "[LLMENV_MCP_CALL] {tool} {us}us req={req_bytes} resp={resp_bytes}"
    ));
}
