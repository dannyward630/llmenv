//! The text of each `[LLMENV_*]` trace line, as pure functions.
//!
//! An emitter builds its line here, so a test can read the exact format. The docs-sync test in
//! `docs_sync.rs` compares each line with `website/docs/trace.md` (#3018).

use std::time::Duration;

/// `[LLMENV_CACHE] <name> <hit|miss> <ms>ms[ <extra>]`
pub(crate) fn cache_line(name: &str, hit: bool, elapsed: Duration, extra: Option<&str>) -> String {
    let status = if hit { "hit" } else { "miss" };
    let ms = elapsed.as_secs_f64() * 1000.0;
    let mut line = format!("[LLMENV_CACHE] {name} {status} {ms:.3}ms");
    if let Some(extra) = extra {
        line.push(' ');
        line.push_str(&crate::util::escape_control(extra));
    }
    line
}

/// `[LLMENV_SPAWN] <label> <us>us exit=<exit>`
pub(crate) fn spawn_line(label: &str, elapsed: Duration, exit: crate::trace::SpawnExit) -> String {
    use crate::trace::SpawnExit;
    let us = elapsed.as_micros();
    let exit = match exit {
        SpawnExit::Code(code) => code.to_string(),
        SpawnExit::Signal => "signal".to_string(),
        SpawnExit::Timeout => "timeout".to_string(),
        SpawnExit::Error => "spawn-error".to_string(),
        SpawnExit::Detached => "detached".to_string(),
    };
    format!("[LLMENV_SPAWN] {label} {us}us exit={exit}")
}

/// `[LLMENV_LEDGER] <op> <us>us`
pub(crate) fn ledger_update_line(op: &str, elapsed: Duration) -> String {
    format!("[LLMENV_LEDGER] {op} {}us", elapsed.as_micros())
}

/// The fields of the keyed `[LLMENV_LEDGER]` line. A `None` means the step did not run.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LedgerFields {
    pub lock_attempts: Option<u32>,
    pub lock_wait_us: Option<u64>,
    pub read_us: Option<u64>,
    pub write_us: Option<u64>,
    pub bytes: Option<usize>,
    pub state_unsaved: Option<bool>,
}

/// `[LLMENV_LEDGER] <flow> lock_attempts=<n> lock_wait_us=<n> read_us=<n> write_us=<n> bytes=<n>
/// state_unsaved=<bool>`. A field for a step that did not run prints `na`, never a made-up zero.
pub(crate) fn ledger_flow_line(flow: &str, f: LedgerFields) -> String {
    fn show<T: std::fmt::Display>(v: Option<T>) -> String {
        v.map_or_else(|| "na".to_string(), |v| v.to_string())
    }
    let LedgerFields {
        lock_attempts,
        lock_wait_us,
        read_us,
        write_us,
        bytes,
        state_unsaved,
    } = f;
    format!(
        "[LLMENV_LEDGER] {flow} lock_attempts={} lock_wait_us={} read_us={} write_us={} \
         bytes={} state_unsaved={}",
        show(lock_attempts),
        show(lock_wait_us),
        show(read_us),
        show(write_us),
        show(bytes),
        show(state_unsaved),
    )
}

/// `[LLMENV_MCP_CALL] <name> <us>us req=<n> resp=<n>[ status=<code>]`
pub(crate) fn mcp_call_line(
    name: &str,
    elapsed: Duration,
    req_bytes: u64,
    resp_bytes: u64,
    status: Option<u16>,
) -> String {
    let us = elapsed.as_micros();
    let mut line = format!("[LLMENV_MCP_CALL] {name} {us}us req={req_bytes} resp={resp_bytes}");
    if let Some(code) = status {
        line.push_str(&format!(" status={code}"));
    }
    line
}

/// `[LLMENV_MATERIALIZE] <adapter> written=<n> skipped=<n> bytes=<n> <us>us`
pub(crate) fn materialize_line(
    adapter: &str,
    wrote: llmenv_paths::WriteStats,
    elapsed: Duration,
) -> String {
    let (written, skipped, bytes) = (wrote.files, wrote.skipped, wrote.bytes);
    let us = elapsed.as_micros();
    format!(
        "[LLMENV_MATERIALIZE] {adapter} written={written} skipped={skipped} bytes={bytes} {us}us"
    )
}

/// The numbers of a finished detached job.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DetachedStats {
    pub wall_us: u64,
    pub cpu_us: u64,
    pub mcp_calls: u64,
    pub exit: i32,
}

/// The longest request id a detached line prints.
const MAX_REQUEST_ID_CHARS: usize = 128;

/// `[LLMENV_DETACHED] <kind> <us>us cpu_us=<n> mcp_calls=<n> exit=<code>[ request_id=<id>]`
pub(crate) fn detached_line(kind: &str, s: DetachedStats, request_id: Option<&str>) -> String {
    let DetachedStats {
        wall_us,
        cpu_us,
        mcp_calls,
        exit,
    } = s;
    let mut line = format!(
        "[LLMENV_DETACHED] {kind} {wall_us}us cpu_us={cpu_us} mcp_calls={mcp_calls} exit={exit}"
    );
    if let Some(id) = request_id.filter(|id| !id.is_empty()) {
        // A request id is a short hash. The cap keeps one line short if a payload carries more.
        let id: String = crate::util::escape_control(id)
            .chars()
            .take(MAX_REQUEST_ID_CHARS)
            .collect();
        line.push_str(&format!(" request_id={id}"));
    }
    line
}
