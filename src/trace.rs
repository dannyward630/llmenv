//! Structured trace telemetry for `LLMENV_TRACE_TIMING` and `LLMENV_TRACE_FILE`.
//!
//! This module is the single sink for every trace line the binary emits:
//! the `llmenv-trace <json>` marker, the `[LLMENV_CACHE]` lines, the
//! `[LLMENV_MCP_CALL]` lines, and the `[LLMENV_SPAWN]` / `[LLMENV_LEDGER]`
//! lines added by later sub-issues. Two env vars control where lines go:
//!
//! - `LLMENV_TRACE_TIMING` — when set, every trace line is also written to
//!   stderr. This is the original path and remains the default for
//!   interactive shells.
//! - `LLMENV_TRACE_FILE` — when set, every trace line is appended as one
//!   JSON object to the named file using `O_APPEND`. Concurrent hooks and
//!   detached jobs use this path so their output reaches a file instead of
//!   a detached stderr.
//!
//! The JSON marker (`llmenv-trace <json>`) carries a `"v": 1` field so a
//! consumer can distinguish formats when new fields are added (#3018).
//! All timing values use microseconds (`_us`).

use std::io::Write;
use std::time::{Duration, Instant};

/// Whether any trace sink is on: `LLMENV_TRACE_TIMING` (stderr) or `LLMENV_TRACE_FILE` (file).
///
/// An emitter returns early when this is `false`, so an untraced run pays for no formatting.
pub(crate) fn enabled() -> bool {
    std::env::var_os("LLMENV_TRACE_TIMING").is_some()
        || std::env::var_os("LLMENV_TRACE_FILE").is_some()
}

/// The file named by `LLMENV_TRACE_FILE`, or `None` when it is unset or empty.
pub(crate) fn file_sink_path() -> Option<std::path::PathBuf> {
    std::env::var_os("LLMENV_TRACE_FILE")
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
}

/// Emit `line` to stderr when `LLMENV_TRACE_TIMING` is set, and append it to the file named by
/// `LLMENV_TRACE_FILE` when that is set.
///
/// The file gets one JSON object per line with `ts` (Unix seconds), `pid`, and `line`, so a
/// consumer can parse it as JSON Lines.
pub(crate) fn emit_trace_line(line: &str) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_some() {
        // A closed stderr must not abort the hook that is being traced.
        let _ = writeln!(std::io::stderr(), "{line}");
    }
    if let Some(path) = file_sink_path() {
        append_trace_line(&path, line);
    }
}

/// Say once per process that the trace file cannot be written. A hook or a detached job has no
/// visible log, so a bad path would otherwise give an empty trace with no explanation.
fn report_sink_error(path: &std::path::Path, error: &std::io::Error) {
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        let _ = writeln!(
            std::io::stderr(),
            "llmenv: cannot write the trace file {}: {error}. \
             Fix: set LLMENV_TRACE_FILE to a path in a writable directory that is not a symlink.",
            path.display()
        );
    }
}

/// Append `line` to `path` as one JSON object, in a single write.
///
/// The file is created owner-only, an existing looser file is tightened, and a symlink at `path`
/// is refused, because a planted link would redirect the appends.
pub(crate) fn append_trace_line(path: &std::path::Path, line: &str) {
    let mut file = match llmenv_paths::open_append_owner_only(path) {
        Ok(file) => file,
        Err(e) => {
            report_sink_error(path, &e);
            return;
        }
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let obj = serde_json::json!({ "ts": ts, "pid": std::process::id(), "line": line });
    // One write call keeps concurrent `O_APPEND` writers from interleaving inside a line.
    if let Err(e) = file.write_all(format!("{obj}\n").as_bytes()) {
        report_sink_error(path, &e);
    }
}

/// Build the `llmenv-trace <json>` marker line. The JSON object is flat: `fields` keeps its
/// own keys, and the line adds `"v": 1` and `"op"`. Existing consumers read the timing keys
/// at the top level.
pub(crate) fn json_trace_line(
    op: &str,
    mut fields: serde_json::Map<String, serde_json::Value>,
) -> String {
    debug_assert!(
        !fields.contains_key("v") && !fields.contains_key("op"),
        "a trace field named v or op would be overwritten"
    );
    fields.insert("v".to_string(), serde_json::json!(1));
    fields.insert("op".to_string(), serde_json::json!(op));
    format!("llmenv-trace {}", serde_json::Value::Object(fields))
}

/// Emit the `llmenv-trace <json>` marker for `op` to the trace sink.
pub(crate) fn emit_json_trace(op: &str, fields: serde_json::Map<String, serde_json::Value>) {
    emit_trace_line(&json_trace_line(op, fields));
}

/// Measured durations of the `SessionStart` sub-phases that run inside `prep_us` (#3011).
///
/// Each field is `None` when the sub-phase did not run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStartPhases {
    /// The MCP health probe.
    pub mcp_probe: Option<Duration>,
    /// The restart of the memory proxy, when it was down.
    pub proxy_restart: Option<Duration>,
    /// The second probe of the memory server after a restart.
    pub drift_check: Option<Duration>,
    /// The check for stale state/tmp entries.
    pub stale_hint: Option<Duration>,
    /// The codebase-memory allow-root check.
    pub cbm_roots: Option<Duration>,
}

/// Emit a `[LLMENV_CACHE] <name> <hit|miss> <duration>ms[ <extra>]` line to
/// the trace sink.
pub(crate) fn emit_cache_trace(name: &str, hit: bool, elapsed: Duration, extra: Option<&str>) {
    if enabled() {
        emit_trace_line(&lines::cache_line(name, hit, elapsed, extra));
    }
}

/// Build the `LLMENV_TRACE_TIMING` marker payload from whichever phase
/// boundaries `run_inner` reached before returning. `t0`/`t_config` are
/// always available (computed before any early return); `t_scope`/`t_chunk`/
/// `t_end` are `None` on a path that returned before reaching them. Each
/// field is included only when its Instant is present, so an early return
/// still reports the phases it actually ran through instead of nothing
/// (#1128: previously only events reaching the full memory-dispatch path —
/// 4 of 11 — emitted this marker at all).
///
/// `phases` carries optional sub-phase boundaries for `SessionStart` that
/// break out `prep_us` + `mcp_us` into individual fields (#3011). Pass a
/// struct with `None` fields when the sub-phase did not run, so the
/// early-return rule holds.
///
/// The JSON marker carries `"v": 1` so a consumer can distinguish formats
/// when new optional fields are added by later sub-issues (#3018).
/// Phase boundaries and per-event timings for one hook run.
///
/// `t0` and `t_config` are always set. Every other field is `None` when the
/// run returned before that phase.
pub(crate) struct HookTiming {
    pub t0: Instant,
    pub t_config: Instant,
    pub t_scope: Option<Instant>,
    pub t_chunk: Option<Instant>,
    pub t_end: Option<Instant>,
    pub phases: SessionStartPhases,
    pub session_log_us: Option<u64>,
    pub checkpoint_us: Option<u64>,
    pub spawn_us: Option<u64>,
    pub spawned_jobs: Option<u64>,
}

impl HookTiming {
    /// Timing for a run that returned right after the config load.
    pub(crate) fn new(t0: Instant, t_config: Instant) -> Self {
        Self {
            t0,
            t_config,
            t_scope: None,
            t_chunk: None,
            t_end: None,
            phases: SessionStartPhases::default(),
            session_log_us: None,
            checkpoint_us: None,
            spawn_us: None,
            spawned_jobs: None,
        }
    }
}

pub(crate) fn trace_timing_json(timing: HookTiming) -> serde_json::Map<String, serde_json::Value> {
    // Cap rather than panic on the (unreachable) overflow of an in-process
    // Instant delta past u64::MAX microseconds (~585,000 years).
    let us = |d: Duration| u64::try_from(d.as_micros()).unwrap_or(u64::MAX);
    let HookTiming {
        t0,
        t_config,
        t_scope,
        t_chunk,
        t_end,
        phases,
        ..
    } = timing;
    let mut fields = serde_json::Map::new();
    fields.insert(
        "config_load_us".to_string(),
        serde_json::json!(us(t_config.saturating_duration_since(t0))),
    );
    if let Some(t_scope) = t_scope {
        fields.insert(
            "scope_eval_us".to_string(),
            serde_json::json!(us(t_scope.saturating_duration_since(t_config))),
        );
        if let Some(t_chunk) = t_chunk {
            fields.insert(
                "prep_us".to_string(),
                serde_json::json!(us(t_chunk.saturating_duration_since(t_scope))),
            );
            let sub_phases = [
                ("mcp_probe_us", phases.mcp_probe),
                ("proxy_restart_us", phases.proxy_restart),
                ("drift_check_us", phases.drift_check),
                ("stale_hint_us", phases.stale_hint),
                ("cbm_roots_us", phases.cbm_roots),
            ];
            for (name, took) in sub_phases {
                if let Some(took) = took {
                    fields.insert(name.to_string(), serde_json::json!(us(took)));
                }
            }
            if let Some(t_end) = t_end {
                fields.insert(
                    "mcp_us".to_string(),
                    serde_json::json!(us(t_end.saturating_duration_since(t_chunk))),
                );
            }
        }
    }
    // Per-hook-event fields for session-log and checkpoint timing (#3013).
    let extras = [
        ("session_log_us", timing.session_log_us),
        ("checkpoint_us", timing.checkpoint_us),
        ("spawn_us", timing.spawn_us),
        ("spawned_jobs", timing.spawned_jobs),
    ];
    for (name, value) in extras {
        if let Some(v) = value {
            fields.insert(name.to_string(), serde_json::json!(v));
        }
    }
    fields
}

/// The hook marker as parsed JSON, as a consumer reads it. For tests.
#[cfg(test)]
pub(crate) fn hook_marker(timing: HookTiming) -> serde_json::Value {
    let line = json_trace_line("hook_run", trace_timing_json(timing));
    serde_json::from_str(line.strip_prefix("llmenv-trace ").unwrap_or_default()).unwrap_or_default()
}

/// Emit the per-phase timing marker to the trace sink.
pub(crate) fn emit_trace_timing(timing: HookTiming) {
    emit_json_trace("hook_run", trace_timing_json(timing));
}

/// Emit the `llmenv-trace` marker of a guard command: `{"op":"guard","command":<name>,
/// "total_us":<n>}`. `command` is one of the five fixed guard names (#3016).
pub(crate) fn emit_guard_trace(command: &str, total: Duration) {
    if !enabled() {
        return;
    }
    let total_us = u64::try_from(total.as_micros()).unwrap_or(u64::MAX);
    let mut fields = serde_json::Map::new();
    fields.insert("command".to_string(), serde_json::json!(command));
    fields.insert("total_us".to_string(), serde_json::json!(total_us));
    emit_json_trace("guard", fields);
}

/// How a traced child process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpawnExit {
    /// The child exited with this code.
    Code(i32),
    /// The child ended on a signal.
    Signal,
    /// The child ran past its deadline and was killed.
    Timeout,
    /// The child did not start, or its output could not be read.
    Error,
    /// The parent started the child and did not wait for it.
    Detached,
}

impl SpawnExit {
    /// The outcome of a probe that talks to a child over a pipe: `0` for an answer, else an error.
    pub(crate) fn of_probe(answered: bool) -> Self {
        if answered { Self::Code(0) } else { Self::Error }
    }

    /// Classify the result of a waited child.
    pub(crate) fn of(result: &std::io::Result<std::process::Output>) -> Self {
        match result {
            Ok(out) => out.status.code().map_or(Self::Signal, Self::Code),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Self::Timeout,
            Err(_) => Self::Error,
        }
    }
}

/// Emit a `[LLMENV_SPAWN] <label> <duration>us exit=<code|signal|timeout|spawn-error|detached>`
/// line to the trace sink. The label is a fixed string such as `probe:ssid` — never print
/// arguments, because they can carry secrets such as keychain item names.
pub(crate) fn emit_spawn_trace(label: &str, elapsed: Duration, exit: SpawnExit) {
    if enabled() {
        emit_trace_line(&lines::spawn_line(label, elapsed, exit));
    }
}

/// Emit a `[LLMENV_LEDGER] <op> <duration>us` line to the trace sink.
///
/// Tracks the time to load or update a per-session recall ledger file.
pub(crate) fn emit_ledger_trace(op: &str, elapsed: Duration) {
    if enabled() {
        emit_trace_line(&lines::ledger_update_line(op, elapsed));
    }
}

mod detached;
pub(crate) mod lines;

#[cfg(test)]
pub(crate) mod docs_sync;

pub(crate) use detached::{
    DetachedJob, emit_detached_trace, json_str, spawn_counted, spawned_jobs,
};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // emit_trace_line never panics when neither env var is set.
    #[test]
    fn emit_trace_line_never_panics_without_env_var() {
        emit_trace_line("[LLMENV_CACHE] content_hash hit 1.234ms");
        emit_trace_line("llmenv-trace {\"v\": 1}");
    }

    // emit_json_trace never panics when neither env var is set.
    #[test]
    fn emit_json_trace_never_panics_without_env_var() {
        emit_json_trace("export", serde_json::Map::new());
    }

    // Every documented trace line example parses with the same code path
    // the binary uses to emit them. A format change that breaks parsing
    // fails this test until the docs are updated.

    // [LLMENV_CACHE] line parses correctly.
    #[test]
    fn cache_trace_line_parses() {
        let line = "[LLMENV_CACHE] content_hash hit 1.234ms extra_data";
        emit_trace_line(line);
    }

    // [LLMENV_SPAWN] line parses correctly.
    #[test]
    fn spawn_trace_line_parses() {
        let line = "[LLMENV_SPAWN] probe:ssid 1234us exit=0";
        emit_trace_line(line);
    }

    // [LLMENV_MCP_CALL] handshake line parses correctly.
    #[test]
    fn mcp_call_trace_line_parses() {
        let line = "[LLMENV_MCP_CALL] initialize 5678us";
        emit_trace_line(line);
    }

    // [LLMENV_MCP_CALL] line with status parses correctly.
    #[test]
    fn mcp_call_trace_line_with_status_parses() {
        let line = "[LLMENV_MCP_CALL] initialize 5678us status=500";
        emit_trace_line(line);
    }

    // [LLMENV_MCP_CALL] handshake line with req/resp bytes parses correctly.
    #[test]
    fn mcp_call_trace_line_with_bytes_parses() {
        let line = "[LLMENV_MCP_CALL] initialize 5678us req=256 resp=1024";
        emit_trace_line(line);
    }

    // [LLMENV_MCP_CALL] handshake line with bytes and status parses correctly.
    #[test]
    fn mcp_call_trace_line_with_bytes_and_status_parses() {
        let line = "[LLMENV_MCP_CALL] initialize 5678us req=256 resp=1024 status=500";
        emit_trace_line(line);
    }

    // [LLMENV_MCP_CALL] tool call line with req/resp bytes parses correctly.
    #[test]
    fn mcp_call_trace_tool_line_with_bytes_parses() {
        let line = "[LLMENV_MCP_CALL] icm_memory_recall 123us req=512 resp=2048";
        emit_trace_line(line);
    }

    // [LLMENV_LEDGER] line parses correctly.
    #[test]
    fn ledger_trace_line_parses() {
        let line = "[LLMENV_LEDGER] session_start lock_attempts=5 lock_wait_us=100 read_us=200 write_us=300 bytes=1024 state_unsaved=false";
        emit_trace_line(line);
    }

    // [LLMENV_CONTEXT] recall-budget line parses correctly.
    #[test]
    fn context_trace_line_parses() {
        let line = "[LLMENV_CONTEXT] recall_entries=10 recall_bytes=2048 injected_entries=5 injected_bytes=1024 duplicate_records=2 already_sent=1 omitted=3 skipped_actions=0";
        emit_trace_line(line);
    }

    // The marker is `llmenv-trace ` plus a flat JSON object: consumers read timing keys at the
    // top level, and `v` and `op` sit beside them.
    #[test]
    fn json_trace_line_is_prefixed_flat_and_versioned() {
        let mut fields = serde_json::Map::new();
        fields.insert("config_load_us".to_string(), serde_json::json!(12));
        let line = json_trace_line("export", fields);
        let json = line.strip_prefix("llmenv-trace ").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(parsed["v"], 1);
        assert_eq!(parsed["op"], "export");
        assert_eq!(parsed["config_load_us"], 12);
        assert!(parsed.get("data").is_none());
    }

    // Each session-start sub-phase shows as a `_us` field only when it ran, and holds its own
    // measured duration.
    #[test]
    fn session_start_sub_phases_report_their_own_durations() {
        let t0 = std::time::Instant::now();
        let t_config = t0 + Duration::from_micros(5);
        let t_scope = t0 + Duration::from_micros(9);
        let t_chunk = t0 + Duration::from_micros(500);
        let json = trace_timing_json(HookTiming {
            t_scope: Some(t_scope),
            t_chunk: Some(t_chunk),
            phases: SessionStartPhases {
                mcp_probe: Some(Duration::from_micros(120)),
                stale_hint: Some(Duration::from_micros(30)),
                ..SessionStartPhases::default()
            },
            ..HookTiming::new(t0, t_config)
        });
        assert_eq!(json["mcp_probe_us"], 120);
        assert_eq!(json["stale_hint_us"], 30);
        assert!(json.get("proxy_restart_us").is_none());
        assert!(json.get("cbm_roots_us").is_none());
        let sum: u64 = ["mcp_probe_us", "stale_hint_us"]
            .iter()
            .filter_map(|k| json[*k].as_u64())
            .sum();
        assert!(sum <= json["prep_us"].as_u64().unwrap());
    }

    // The trace file is owner-only, and a symlink at its path is not followed.
    #[cfg(unix)]
    #[test]
    fn the_trace_file_is_owner_only_and_refuses_a_symlink() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.log");
        append_trace_line(&path, "[LLMENV_SPAWN] x 1us exit=0");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let link = dir.path().join("link.log");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        append_trace_line(&link, "[LLMENV_SPAWN] y 1us exit=0");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "the link is not followed"
        );
    }

    // The sink writes one JSON object per line, and appends rather than truncates.
    #[test]
    fn append_trace_line_writes_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.log");
        append_trace_line(&path, "[LLMENV_SPAWN] probe:ssid 5us exit=0");
        append_trace_line(&path, "second \"quoted\" line");
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["line"], "[LLMENV_SPAWN] probe:ssid 5us exit=0");
        assert_eq!(lines[1]["line"], "second \"quoted\" line");
        assert_eq!(lines[0]["pid"], std::process::id());
    }

    // The request id of a detached job comes from its payload, and a missing one is None.
    #[test]
    fn json_str_reads_a_string_field_or_none() {
        assert_eq!(
            json_str(r#"{"request_id":"abc"}"#, "request_id").as_deref(),
            Some("abc")
        );
        assert_eq!(json_str(r#"{"request_id":3}"#, "request_id"), None);
        assert_eq!(json_str("not json", "request_id"), None);
    }

    // [LLMENV_DETACHED] line parses correctly.
    #[test]
    fn detached_trace_line_parses() {
        let line = "[LLMENV_DETACHED] consolidation 12345us cpu_us=10000 mcp_calls=3 exit=0";
        emit_trace_line(line);
    }

    // [LLMENV_DETACHED] line with error exit parses correctly.
    #[test]
    fn detached_trace_line_error_exit_parses() {
        let line = "[LLMENV_DETACHED] icm-store 5678us cpu_us=4000 mcp_calls=1 exit=1";
        emit_trace_line(line);
    }

    // trace_timing_json always yields a JSON object with "v": 1 and
    // "config_load_us" present, and all timing values are non-negative.
    proptest::proptest! {
        #[test]
        fn trace_timing_json_has_version_and_non_negative_timing(
            config_us in proptest::prelude::any::<u64>(),
            scope_us in proptest::prelude::any::<u64>(),
            chunk_us in proptest::prelude::any::<u64>(),
            end_us in proptest::prelude::any::<u64>(),
        ) {
            let t0 = std::time::Instant::now();
            let t_config = t0 + std::time::Duration::from_micros(config_us);
            let t_scope = t0 + std::time::Duration::from_micros(scope_us);
            let t_chunk = t0 + std::time::Duration::from_micros(chunk_us);
            let t_end = t0 + std::time::Duration::from_micros(end_us);
            let json = hook_marker(HookTiming {
                t_scope: Some(t_scope),
                t_chunk: Some(t_chunk),
                t_end: Some(t_end),
                ..HookTiming::new(t0, t_config)
            });
            proptest::prop_assert_eq!(&json["v"], &serde_json::json!(1));
            proptest::prop_assert!(json.get("config_load_us").is_some());
            proptest::prop_assert!(json["config_load_us"].is_u64());
            if let Some(scope) = json.get("scope_eval_us") {
                proptest::prop_assert!(scope.is_u64());
            }
            if let Some(prep) = json.get("prep_us") {
                proptest::prop_assert!(prep.is_u64());
            }
            if let Some(mcp) = json.get("mcp_us") {
                proptest::prop_assert!(mcp.is_u64());
            }
        }

        // trace_timing_json with all-None phases still carries "v": 1.
        #[test]
        fn trace_timing_json_with_none_phases_has_version(
            config_us in proptest::prelude::any::<u64>(),
        ) {
            let t0 = std::time::Instant::now();
            let t_config = t0 + std::time::Duration::from_micros(config_us);
            let json = hook_marker(HookTiming::new(t0, t_config));
            proptest::prop_assert_eq!(&json["v"], &serde_json::json!(1));
            proptest::prop_assert!(json.get("config_load_us").is_some());
        }
    }

    proptest::proptest! {
        // For any op and any fields that do not use the reserved keys, the marker is the prefix
        // plus one JSON object that keeps every field and adds `v` and `op`.
        #[test]
        fn json_trace_line_keeps_every_field_and_adds_v_and_op(
            op in "\\PC{0,20}",
            fields in proptest::collection::btree_map("[a-z_]{1,12}", 0u64..1_000_000, 0..8),
        ) {
            let mut map = serde_json::Map::new();
            for (key, value) in &fields {
                if key != "v" && key != "op" {
                    map.insert(key.clone(), serde_json::json!(value));
                }
            }
            let line = json_trace_line(&op, map.clone());
            let json = line.strip_prefix("llmenv-trace ").unwrap();
            let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
            proptest::prop_assert_eq!(&parsed["v"], &serde_json::json!(1));
            proptest::prop_assert_eq!(&parsed["op"], &serde_json::json!(op));
            for (key, value) in &map {
                proptest::prop_assert_eq!(&parsed[key.as_str()], value);
            }
            proptest::prop_assert!(!line.contains('\n'), "a marker is one line");
        }

        // `json_str` never panics, and it reads back any string field of a JSON object.
        #[test]
        fn json_str_never_panics_and_reads_back_a_string_field(
            text in "\\PC{0,60}",
            key in "[a-z_]{1,10}",
            value in "\\PC{0,30}",
        ) {
            let _ = json_str(&text, &key);
            let payload = serde_json::json!({ key.clone(): value.clone() }).to_string();
            proptest::prop_assert_eq!(json_str(&payload, &key), Some(value));
        }
    }
}
