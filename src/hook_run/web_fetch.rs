//! The ICM store of a WebFetch or WebSearch result (#579).

use std::io::Write;

use serde_json::json;

use super::{
    checkpoint, detached_child_log_path, detached_store, idempotency, json_or_empty,
    redirect_stderr_to_detached_log,
};

/// Tool name constants for WebFetch and WebSearch tools.
const TOOL_NAME_WEBFETCH: &str = "WebFetch";
const TOOL_NAME_WEBSEARCH: &str = "WebSearch";

/// Build ICM memory store arguments for a WebFetch/WebSearch PostToolUse event.
/// Returns `None` if the payload is not a WebFetch/WebSearch tool result.
///
/// # Format
/// The stored memory carries topic `web-fetch` and importance `low` so it decays
/// quickly and can be bulk-cleared via `icm_memory_forget_topic("web-fetch")`.
#[must_use]
pub(super) fn web_fetch_store_args(payload: &serde_json::Value) -> Option<serde_json::Value> {
    let tool_name = payload["tool_name"].as_str()?;
    if tool_name != TOOL_NAME_WEBFETCH && tool_name != TOOL_NAME_WEBSEARCH {
        return None;
    }
    let is_search = tool_name == TOOL_NAME_WEBSEARCH;
    let source_field = if is_search { "query" } else { "url" };
    let source_value = payload["tool_input"][source_field]
        .as_str()
        .unwrap_or("unknown");
    let label = if is_search { "Query" } else { "URL" };
    let response = payload["tool_response"]
        .as_str()
        .map_or_else(|| json_or_empty(&payload["tool_response"]), String::from);
    let needs_indicator = response.chars().count() > 1000;
    let mut truncated: String = response.chars().take(1000).collect();
    if needs_indicator {
        truncated.push_str("... (truncated)");
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let content = format!(
        "{label}: {source_value}\nTool: {tool_name}\nFetched at (epoch): {timestamp}\nContent preview:\n{truncated}"
    );
    // The id is derived from the event, so a re-spawn or a resumed job sends the same one (#2397).
    let session = payload["session_id"].as_str().unwrap_or_default();
    let event = payload["tool_use_id"]
        .as_str()
        .map_or_else(|| timestamp.to_string(), String::from);
    // `content` holds the fetch time, so the id uses its stable parts: with a `tool_use_id` the
    // same tool call gives the same id at any later second.
    let request_id =
        idempotency::request_id(&[session, "icm-store", &event, source_value, &truncated]);
    Some(json!({
        "content": content,
        "topic": "web-fetch",
        "importance": "low",
        detached_store::REQUEST_ID_FIELD: request_id,
        detached_store::REQUEST_KEY_FIELD: session,
    }))
}

/// Handle PostToolUse for WebFetch/WebSearch by spawning a detached child
/// that stores the fetched content in ICM with fast-falloff memory
/// (importance: low, topic: web-fetch). The hook returns immediately instead of
/// blocking on the MCP round trip. Best-effort — failures are logged at debug
/// level and never propagated to the caller. The child's stderr goes to the
/// shared bounded log rather than `/dev/null` so its own failures are
/// diagnosable (#1133).
///
/// Returns the spawned [`std::process::Child`] purely so callers such as
/// tests can reap it (#1095) — production intentionally drops it unwaited,
/// identical to the previous behavior, since the child is
/// process-group-detached and outlives this process regardless.
pub(super) fn handle_web_fetch_post_tool_use(
    payload: &serde_json::Value,
) -> Option<std::process::Child> {
    handle_web_fetch_in(checkpoint::spawner_state_dir().as_deref(), payload)
}

/// The `llmenv icm-store` child, before its stderr log and process group are set.
pub(super) fn icm_store_command(
    exe: std::path::PathBuf,
    checkpoint: Option<&std::path::Path>,
) -> std::process::Command {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("icm-store")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null());
    if let Some(path) = checkpoint {
        cmd.arg("--checkpoint").arg(path);
    }
    cmd
}

/// [`handle_web_fetch_post_tool_use`] with the state dir for the checkpoint given (#2396).
pub(super) fn handle_web_fetch_in(
    state_dir: Option<&std::path::Path>,
    payload: &serde_json::Value,
) -> Option<std::process::Child> {
    let args = web_fetch_store_args(payload)?;
    let Ok(payload_json) = serde_json::to_string(&args) else {
        tracing::error!("icm-store: failed to serialize store args");
        return None;
    };
    // Spawns this same binary as a detached child. The path is this process's own image, not input.
    // nosemgrep: rust.lang.security.current-exe.current-exe
    let Ok(exe) = std::env::current_exe() else {
        tracing::error!("icm-store: cannot resolve current_exe for detached store");
        return None;
    };
    let checkpoint = checkpoint::begin(
        state_dir,
        checkpoint::JobKind::IcmStore,
        &args,
        payload["session_id"].as_str(),
    );
    let mut cmd = icm_store_command(exe, checkpoint.as_deref());
    redirect_stderr_to_detached_log(&mut cmd, detached_child_log_path());
    crate::mcp::proxy::detach_process_group(&mut cmd);
    let Ok(mut child) = cmd.spawn() else {
        tracing::error!("icm-store: failed to spawn detached store child");
        return None;
    };
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(payload_json.as_bytes())
    {
        tracing::error!("icm-store: failed to pipe args to detached child: {e}");
    }
    // Not waited on by the caller: the child is process-group-detached and
    // outlives us.
    Some(child)
}
