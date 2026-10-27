//! MCP handshake and tool-call trace telemetry.
//!
//! These functions emit `[LLMENV_MCP_CALL]` lines when
//! `LLMENV_TRACE_TIMING` is set. They are separated from `trace.rs` so that
//! `trace.rs` stays near its line cap.

use std::time::Duration;

use crate::trace::emit_trace_line;

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
