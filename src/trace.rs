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

/// Emit `line` to stderr and, when `LLMENV_TRACE_FILE` is set, also append
/// it as a JSON object to the trace file.
///
/// The JSON object carries `ts` (ISO 8601), `pid`, and `line` so a consumer
/// can reconstruct the original text while still being able to parse the
/// file as JSON Lines.
fn emit_trace_line(line: &str) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_some() {
        eprintln!("{line}");
    }
    if let Some(path) = std::env::var_os("LLMENV_TRACE_FILE") {
        let path = std::path::PathBuf::from(path);
        if let Ok(mut f) = open_trace_file(&path) {
            let ts = current_timestamp();
            let pid = std::process::id();
            let obj = serde_json::json!({
                "ts": ts,
                "pid": pid,
                "line": line,
            });
            let _ = writeln!(f, "{}", obj);
            let _ = f.flush();
        }
    }
}

/// Emit a JSON trace object with `op` and the supplied fields. The object
/// also carries `ts`, `pid`, and `"v": 1`.
fn emit_json_trace(op: &str, fields: serde_json::Value) {
    let obj = serde_json::json!({
        "v": 1,
        "op": op,
        "ts": current_timestamp(),
        "pid": std::process::id(),
        "data": fields,
    });
    emit_trace_line(&obj.to_string());
}

/// Emit a `[LLMENV_CACHE] <name> <hit|miss> <duration>ms[ <extra>]` line to
/// the trace sink.
pub(crate) fn emit_cache_trace(name: &str, hit: bool, elapsed: Duration, extra: Option<&str>) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
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
/// The JSON marker carries `"v": 1` so a consumer can distinguish formats
/// when new optional fields are added by later sub-issues (#3018).
pub(crate) fn trace_timing_json(
    t0: Instant,
    t_config: Instant,
    t_scope: Option<Instant>,
    t_chunk: Option<Instant>,
    t_end: Option<Instant>,
) -> serde_json::Value {
    // Cap rather than panic on the (unreachable) overflow of an in-process
    // Instant delta past u64::MAX microseconds (~585,000 years).
    let us = |d: Duration| u64::try_from(d.as_micros()).unwrap_or(u64::MAX);
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
            if let Some(t_end) = t_end {
                fields.insert(
                    "mcp_us".to_string(),
                    serde_json::json!(us(t_end.saturating_duration_since(t_chunk))),
                );
            }
        }
    }
    serde_json::Value::Object(fields)
}

/// Emit the per-phase timing marker to the trace sink.
pub(crate) fn emit_trace_timing(
    t0: Instant,
    t_config: Instant,
    t_scope: Option<Instant>,
    t_chunk: Option<Instant>,
    t_end: Option<Instant>,
) {
    emit_json_trace(
        "hook_run",
        trace_timing_json(t0, t_config, t_scope, t_chunk, t_end),
    );
}

/// Emit a `[LLMENV_SPAWN] <label> <duration>us exit=<code|timeout|spawn-error>` line to the
/// trace sink. The label is a fixed string such as `probe:ssid` — never print arguments,
/// because they can carry secrets such as keychain item names.
pub(crate) fn emit_spawn_trace(label: &str, elapsed: Duration, exit: Option<i32>) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    let us = elapsed.as_micros();
    let exit_str = match exit {
        Some(code) => format!("exit={code}"),
        None => "exit=spawn-error".to_string(),
    };
    emit_trace_line(&format!("[LLMENV_SPAWN] {label} {us}us {exit_str}"));
}

/// Emit a `[LLMENV_MCP_CALL] initialize <us>us` or
/// `[LLMENV_MCP_CALL] notifications/initialized <us>us` line to the trace sink.
/// The leading `<tool> <us>us` fields are kept because consumers already
/// parse them.
pub(crate) fn emit_mcp_handshake_trace(method: &str, elapsed: Duration, status: Option<u16>) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    let us = elapsed.as_micros();
    let status_str = match status {
        Some(code) => format!(" status={code}"),
        None => String::new(),
    };
    emit_trace_line(&format!("[LLMENV_MCP_CALL] {method} {us}us{status_str}"));
}

/// Emit a `[LLMENV_MCP_CALL] <tool> <duration>us` line to the trace sink.
pub(crate) fn emit_mcp_call_trace(tool: &str, elapsed: Duration) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    let us = elapsed.as_micros();
    emit_trace_line(&format!("[LLMENV_MCP_CALL] {tool} {us}us"));
}

/// Emit a `llmenv-trace {"op":"export",...}` line with per-phase timing.
/// The fields are optional — a phase that did not run is left out.
pub(crate) fn emit_export_trace(
    t0: Instant,
    t_config: Instant,
    t_env: Option<Instant>,
    t_active: Option<Instant>,
    t_manifest: Option<Instant>,
    t_adapters: Option<Instant>,
    t_render: Option<Instant>,
) {
    let us = |d: Duration| u64::try_from(d.as_micros()).unwrap_or(u64::MAX);
    let mut fields = serde_json::Map::new();
    fields.insert(
        "config_load_us".to_string(),
        serde_json::json!(us(t_config.saturating_duration_since(t0))),
    );
    if let Some(t_env) = t_env {
        fields.insert(
            "detect_env_us".to_string(),
            serde_json::json!(us(t_env.saturating_duration_since(t_config))),
        );
    }
    if let Some(t_active) = t_active {
        fields.insert(
            "evaluate_active_us".to_string(),
            serde_json::json!(us(
                t_active.saturating_duration_since(t_env.unwrap_or(t_config))
            )),
        );
    }
    if let Some(t_manifest) = t_manifest {
        fields.insert(
            "build_manifest_us".to_string(),
            serde_json::json!(us(
                t_manifest.saturating_duration_since(t_active.unwrap_or(t_env.unwrap_or(t_config)))
            )),
        );
    }
    if let Some(t_adapters) = t_adapters {
        fields.insert(
            "materialize_adapters_us".to_string(),
            serde_json::json!(us(t_adapters.saturating_duration_since(
                t_manifest.unwrap_or(t_active.unwrap_or(t_env.unwrap_or(t_config)))
            ))),
        );
    }
    if let Some(t_render) = t_render {
        fields.insert(
            "render_vars_us".to_string(),
            serde_json::json!(us(t_render
                .saturating_duration_since(t_adapters.unwrap_or(
                    t_manifest.unwrap_or(t_active.unwrap_or(t_env.unwrap_or(t_config)))
                )))),
        );
    }
    emit_json_trace("export", serde_json::Value::Object(fields));
}

/// Emit a `[LLMENV_MATERIALIZE] <adapter> written=<n> unchanged=<n> removed=<n> bytes=<n> <us>us`
/// line to the trace sink.
///
/// The function lists all files in the adapter cache directory, excludes metadata
/// files (`llmenv-status.json`, `.llmenv-manifest.json`), and compares the result
/// against the manifest to compute written / unchanged / removed counts and the
/// total bytes of written files.
fn emit_materialize_trace(
    adapter_name: &str,
    cache_path: &std::path::Path,
    manifest_file_names: &[&str],
    written_file_names: &[&str],
    elapsed: Duration,
) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    // Metadata files that should not count toward materialize stats.
    let meta: &[&str] = &["llmenv-status.json", ".llmenv-manifest.json"];
    // All files in the cache directory, excluding metadata, as owned names.
    let cache_names: Vec<String> = match std::fs::read_dir(cache_path) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if meta.contains(&name.as_str()) {
                    return None;
                }
                Some(name)
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let mut written_bytes: u64 = 0;
    for name in written_file_names {
        let path = cache_path.join(*name);
        if let Ok(fmeta) = std::fs::metadata(&path)
            && fmeta.is_file()
        {
            written_bytes += fmeta.len();
        }
    }
    // Files in cache but not in manifest = removed.
    let removed: usize = cache_names
        .iter()
        .filter(|n| !manifest_file_names.contains(&n.as_str()))
        .count();
    let unchanged: usize = manifest_file_names
        .iter()
        .filter(|n| {
            let path = cache_path.join(**n);
            std::fs::metadata(&path).is_ok_and(|m| m.is_file())
        })
        .count();
    let written = written_file_names.len();
    let us = elapsed.as_micros();
    emit_trace_line(&format!(
        "[LLMENV_MATERIALIZE] {adapter_name} \
         written={written} unchanged={unchanged} removed={removed} \
         bytes={written_bytes} {us}us"
    ));
}

/// Record materialize file counts for one adapter and emit the trace line.
///
/// This is a thin wrapper that extracts file names from the manifest and
/// adapter-owned paths, then calls `emit_materialize_trace`. Call this
/// after `adapter.materialize()` returns.
pub(crate) fn record_materialize(
    adapter: &dyn crate::adapter::AgentAdapter,
    cache_path: &std::path::Path,
    manifest: &crate::merge::MergedManifest,
    adapter_owned: &[std::path::PathBuf],
    elapsed: Duration,
) {
    // Collect into owned strings to avoid lifetime issues with iterator temporaries.
    let manifest_names: Vec<String> = manifest
        .files
        .keys()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let written_names: Vec<String> = adapter_owned
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    emit_materialize_trace(
        adapter.name(),
        cache_path,
        // Safe: file names from the manifest and adapter are always valid UTF-8 on POSIX.
        &manifest_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        &written_names.iter().map(String::as_str).collect::<Vec<_>>(),
        elapsed,
    );
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
}
