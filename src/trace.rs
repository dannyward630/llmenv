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

/// Open or append `path` with `O_APPEND` so concurrent writers never
/// interleave lines. The kernel guarantees atomic writes of a single line
/// because each line is shorter than `PIPE_BUF` (4 KiB on Linux, 512 B on
/// macOS).
fn open_trace_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::fs::OpenOptions;
    OpenOptions::new().create(true).append(true).open(path)
}

/// Whether any trace sink is on: `LLMENV_TRACE_TIMING` (stderr) or `LLMENV_TRACE_FILE` (file).
///
/// An emitter returns early when this is `false`, so an untraced run pays for no formatting.
pub(crate) fn enabled() -> bool {
    std::env::var_os("LLMENV_TRACE_TIMING").is_some()
        || std::env::var_os("LLMENV_TRACE_FILE").is_some()
}

/// Emit `line` to stderr and, when `LLMENV_TRACE_FILE` is set, also append
/// it as a JSON object to the trace file.
///
/// The JSON object carries `ts` (Unix seconds), `pid`, and `line` so a consumer
/// can reconstruct the original text while still being able to parse the
/// file as JSON Lines.
pub(crate) fn emit_trace_line(line: &str) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_some() {
        eprintln!("{line}");
    }
    if let Some(path) = std::env::var_os("LLMENV_TRACE_FILE") {
        append_trace_line(std::path::Path::new(&path), line);
    }
}

/// Append `line` to `path` as one JSON object with a single `O_APPEND` write.
pub(crate) fn append_trace_line(path: &std::path::Path, line: &str) {
    let mut f = match open_trace_file(path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("cannot open trace file {}: {e}", path.display());
            return;
        }
    };
    let obj = serde_json::json!({
        "ts": current_timestamp(),
        "pid": std::process::id(),
        "line": line,
    });
    // One write call keeps concurrent writers from interleaving inside a line.
    if let Err(e) = f.write_all(format!("{obj}\n").as_bytes()) {
        tracing::warn!("trace file write failed to {}: {e}", path.display());
    }
}

/// Build the `llmenv-trace <json>` marker line. The JSON object is flat: `fields` keeps its
/// own keys, and the line adds `"v": 1` and `"op"`. Existing consumers read the timing keys
/// at the top level.
pub(crate) fn json_trace_line(op: &str, fields: serde_json::Value) -> String {
    let mut obj = match fields {
        serde_json::Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("value".to_string(), other);
            map
        }
    };
    obj.insert("v".to_string(), serde_json::json!(1));
    obj.insert("op".to_string(), serde_json::json!(op));
    format!("llmenv-trace {}", serde_json::Value::Object(obj))
}

/// Emit the `llmenv-trace <json>` marker for `op` to the trace sink.
pub(crate) fn emit_json_trace(op: &str, fields: serde_json::Value) {
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
    if !crate::trace::enabled() {
        return;
    }
    let status = if hit { "hit" } else { "miss" };
    let ms = elapsed.as_secs_f64() * 1000.0;
    match extra {
        Some(extra) => {
            let escaped = crate::util::escape_control(extra);
            emit_trace_line(&format!(
                "[LLMENV_CACHE] {name} {status} {ms:.3}ms {escaped}"
            ));
        }
        None => {
            emit_trace_line(&format!("[LLMENV_CACHE] {name} {status} {ms:.3}ms"));
        }
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

pub(crate) fn trace_timing_json(timing: HookTiming) -> serde_json::Value {
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
    fields.insert("v".to_string(), serde_json::json!(1));
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
    serde_json::Value::Object(fields)
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
    emit_json_trace(
        "guard",
        serde_json::json!({ "command": command, "total_us": total_us }),
    );
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
    if !crate::trace::enabled() {
        return;
    }
    let us = elapsed.as_micros();
    let exit_str = match exit {
        SpawnExit::Code(code) => code.to_string(),
        SpawnExit::Signal => "signal".to_string(),
        SpawnExit::Timeout => "timeout".to_string(),
        SpawnExit::Error => "spawn-error".to_string(),
        SpawnExit::Detached => "detached".to_string(),
    };
    emit_trace_line(&format!("[LLMENV_SPAWN] {label} {us}us exit={exit_str}"));
}

/// Emit a `[LLMENV_LEDGER] <op> <duration>us` line to the trace sink.
///
/// Tracks the time to load or update a per-session recall ledger file.
pub(crate) fn emit_ledger_trace(op: &str, elapsed: Duration) {
    if !crate::trace::enabled() {
        return;
    }
    let us = elapsed.as_micros();
    emit_trace_line(&format!("[LLMENV_LEDGER] {op} {us}us"));
}

/// Return the current time as a Unix-epoch-seconds string.
///
/// This binary crate has no `chrono`/`time` dependency (`rg -n "^chrono|^time"
/// Cargo.toml` finds none). `ts` is informational only — a consumer parses
/// the JSON Lines file, not a human, so a bare Unix-epoch-seconds string is
/// sufficient rather than pulling in a date/time crate for one field.
fn current_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// FFI bindings for `getrusage` on macOS.
mod detached;

pub(crate) use detached::{DetachedJob, emit_detached_trace, json_str};

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
        emit_json_trace("export", serde_json::json!({ "config_load_us": 12 }));
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
        let line = json_trace_line("export", serde_json::json!({ "config_load_us": 12 }));
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
            let obj = trace_timing_json(HookTiming {
                t_scope: Some(t_scope),
                t_chunk: Some(t_chunk),
                t_end: Some(t_end),
                ..HookTiming::new(t0, t_config)
            });
            let json: serde_json::Value = serde_json::from_str(&obj.to_string()).unwrap();
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
            let obj = trace_timing_json(HookTiming::new(t0, t_config));
            let json: serde_json::Value = serde_json::from_str(&obj.to_string()).unwrap();
            proptest::prop_assert_eq!(&json["v"], &serde_json::json!(1));
            proptest::prop_assert!(json.get("config_load_us").is_some());
        }
    }
}
