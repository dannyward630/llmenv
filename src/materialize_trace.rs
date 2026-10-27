//! `[LLMENV_MATERIALIZE]` trace line for adapter materialize steps (#3020).

use crate::trace::emit_trace_line;

/// Emit `[LLMENV_MATERIALIZE] <adapter> written=<n> skipped=<n> bytes=<n> <us>us` to the trace sink.
///
/// `written`, `skipped`, and `bytes` count the files the shared owner-only writers wrote or left
/// alone during the step, taken as the difference from `before`. A file that already held the same
/// bytes at the same mode is `skipped`.
pub(crate) fn record_materialize(
    adapter_name: &str,
    before: llmenv_paths::WriteStats,
    elapsed: std::time::Duration,
) {
    if !crate::trace::enabled() {
        return;
    }
    let wrote = llmenv_paths::write_stats_snapshot().since(before);
    let (written, skipped, bytes) = (wrote.files, wrote.skipped, wrote.bytes);
    let us = elapsed.as_micros();
    emit_trace_line(&format!(
        "[LLMENV_MATERIALIZE] {adapter_name} written={written} skipped={skipped} bytes={bytes} {us}us"
    ));
}
