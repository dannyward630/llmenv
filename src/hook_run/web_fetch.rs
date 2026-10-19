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

/// The topic of a fetched page with no known project. ICM matches a project against the topic,
/// so a page of a known project is stored under `web-fetch-<project>` (#2911).
const WEB_FETCH_TOPIC: &str = "web-fetch";

/// Whether `topic` holds fetched pages, with or without a project.
pub(super) fn is_web_fetch_topic(topic: &str) -> bool {
    topic
        .strip_prefix(WEB_FETCH_TOPIC)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
}

fn web_fetch_topic(project: Option<&str>) -> String {
    match project {
        Some(project) if !project.is_empty() => format!("{WEB_FETCH_TOPIC}-{project}"),
        _ => WEB_FETCH_TOPIC.to_string(),
    }
}

/// Build ICM memory store arguments for a WebFetch/WebSearch PostToolUse event.
/// Returns `None` if the payload is not a WebFetch/WebSearch tool result.
///
/// # Format
/// The stored memory carries topic `web-fetch-<project>` (or `web-fetch` when `project` is
/// `None`) and importance `low`, so it decays quickly and can be bulk-cleared via
/// `icm_memory_forget_topic`.
#[must_use]
pub(super) fn web_fetch_store_args(
    payload: &serde_json::Value,
    project: Option<&str>,
) -> Option<serde_json::Value> {
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
        "topic": web_fetch_topic(project),
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
    let args = web_fetch_store_args(payload, session_project(payload).as_deref())?;
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

/// The project of the session that fetched the page. A page with no project is stored, but no
/// recall across projects returns it.
fn session_project(payload: &serde_json::Value) -> Option<String> {
    let cwd = payload["cwd"]
        .as_str()
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    crate::memory::project::session_project(&cwd)
        .inspect_err(|e| tracing::error!("web fetch stored with no project: {e:#}"))
        .ok()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    fn fetch() -> serde_json::Value {
        json!({"tool_name": "WebFetch", "tool_input": {"url": "https://a"}, "tool_response": "x"})
    }

    #[test]
    fn a_page_is_stored_under_the_topic_of_its_project() {
        let args = web_fetch_store_args(&fetch(), Some("proj-a")).unwrap();
        assert_eq!(args["topic"], "web-fetch-proj-a");
    }

    #[test]
    fn a_page_with_no_project_keeps_the_plain_topic() {
        for project in [None, Some("")] {
            let args = web_fetch_store_args(&fetch(), project).unwrap();
            assert_eq!(args["topic"], "web-fetch", "{project:?}");
        }
    }

    #[test]
    fn only_the_web_fetch_topics_count_as_fetched_pages() {
        for topic in ["web-fetch", "web-fetch-llmenv", "web-fetch-a-b"] {
            assert!(is_web_fetch_topic(topic), "{topic}");
        }
        for topic in [
            "web-fetcher",
            "context-web-fetch",
            "web",
            "",
            "decisions-llmenv",
        ] {
            assert!(!is_web_fetch_topic(topic), "{topic}");
        }
    }

    #[test]
    fn the_project_comes_from_the_payload_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("named-folder");
        std::fs::create_dir(&cwd).unwrap();
        let payload = json!({"cwd": cwd.display().to_string()});
        assert_eq!(session_project(&payload).as_deref(), Some("named-folder"));
    }

    #[test]
    fn web_fetch_store_args_extracts_url_and_summary() {
        let payload = json!({
            "tool_name": "WebFetch",
            "tool_input": {"url": "https://example.com"},
            "tool_response": "# Hello\n\nThis is fetched content",
        });
        let args = web_fetch_store_args(&payload, None).expect("should detect WebFetch");
        assert_eq!(args["topic"], "web-fetch");
        assert_eq!(args["importance"], "low");
        let content = args["content"].as_str().unwrap();
        assert!(content.contains("https://example.com"), "url in content");
        assert!(content.contains("WebFetch"), "tool name in content");
        assert!(
            content.contains("Fetched at (epoch)"),
            "timestamp in content"
        );
        assert!(content.contains("Hello"), "content preview in content");
    }

    #[test]
    fn web_fetch_request_id_is_stable_per_tool_call_and_keyed_by_session() {
        let payload = |tool_use_id: &str, url: &str| {
            json!({
                "session_id": "sess-1", "tool_use_id": tool_use_id, "tool_name": "WebFetch",
                "tool_input": {"url": url}, "tool_response": "body",
            })
        };
        let a = web_fetch_store_args(&payload("tu1", "https://a"), None).expect("args");
        let again = web_fetch_store_args(&payload("tu1", "https://a"), None).expect("args");
        assert_eq!(
            a[detached_store::REQUEST_ID_FIELD],
            again[detached_store::REQUEST_ID_FIELD]
        );
        assert_eq!(a[detached_store::REQUEST_KEY_FIELD], "sess-1");
        let other_call = web_fetch_store_args(&payload("tu2", "https://a"), None).expect("args");
        assert_ne!(
            a[detached_store::REQUEST_ID_FIELD],
            other_call[detached_store::REQUEST_ID_FIELD]
        );
        let other_url = web_fetch_store_args(&payload("tu1", "https://b"), None).expect("args");
        assert_ne!(
            a[detached_store::REQUEST_ID_FIELD],
            other_url[detached_store::REQUEST_ID_FIELD]
        );
    }

    #[test]
    fn web_fetch_store_args_supports_web_search() {
        let payload = json!({
            "tool_name": "WebSearch",
            "tool_input": {"query": "rust programming"},
            "tool_response": "Search results here",
        });
        let args = web_fetch_store_args(&payload, None).expect("should detect WebSearch");
        assert_eq!(args["topic"], "web-fetch");
        let content = args["content"].as_str().unwrap();
        assert!(content.contains("WebSearch"));
        assert!(content.contains("Search results"));
        assert!(content.contains("Query: rust programming"));
        assert!(content.contains("Tool: WebSearch"));
    }

    #[test]
    fn web_fetch_store_args_ignores_non_web_fetch_tools() {
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "ls"},
            "tool_response": "result",
        });
        assert!(
            web_fetch_store_args(&payload, None).is_none(),
            "non-WebFetch tool should return None"
        );
    }

    #[test]
    fn web_fetch_store_args_handles_missing_url_or_tool_input() {
        // Missing url key within tool_input.
        let payload = json!({
            "tool_name": "WebFetch",
            "tool_input": {},
            "tool_response": "content",
        });
        let args = web_fetch_store_args(&payload, None).expect("should handle missing url");
        let content = args["content"].as_str().unwrap();
        assert!(content.contains("unknown"), "should fall back to 'unknown'");

        // Missing entire tool_input key — same serde_json Null path.
        let payload2 = json!({
            "tool_name": "WebFetch",
            "tool_response": "content",
        });
        let args2 =
            web_fetch_store_args(&payload2, None).expect("should handle missing tool_input");
        let content2 = args2["content"].as_str().unwrap();
        assert!(
            content2.contains("unknown"),
            "should fall back to 'unknown'"
        );
    }

    #[test]
    fn web_fetch_store_args_handles_empty_response() {
        let payload = json!({
            "tool_name": "WebFetch",
            "tool_input": {"url": "https://example.com"},
            "tool_response": "",
        });
        let args = web_fetch_store_args(&payload, None).expect("should handle empty response");
        let content = args["content"].as_str().unwrap();
        assert!(
            content.contains("Content preview:\n"),
            "empty content after preview header"
        );
    }

    #[test]
    fn web_fetch_store_args_truncates_long_content() {
        let long = "x".repeat(2000);
        let payload = json!({
            "tool_name": "WebFetch",
            "tool_input": {"url": "https://example.com"},
            "tool_response": long,
        });
        let args = web_fetch_store_args(&payload, None).expect("should handle long content");
        let content = args["content"].as_str().unwrap();
        let preview = content.split("Content preview:\n").nth(1).unwrap_or("");
        assert!(
            preview.ends_with("... (truncated)"),
            "truncation indicator should be present, got: {preview:?}"
        );
        let truncated = preview.strip_suffix("... (truncated)").unwrap_or(preview);
        assert!(
            truncated.len() <= 1000,
            "truncated content should be at most 1000 chars, got {}",
            truncated.len()
        );
    }

    #[test]
    fn web_fetch_store_args_returns_none_for_null_payload() {
        assert!(web_fetch_store_args(&serde_json::Value::Null, None).is_none());
    }

    #[test]
    fn web_fetch_store_args_returns_none_for_missing_tool_name() {
        let payload = json!({
            "tool_input": {"url": "https://example.com"},
            "tool_response": "content",
        });
        assert!(web_fetch_store_args(&payload, None).is_none());
    }

    #[test]
    fn web_fetch_store_args_handles_object_tool_response() {
        let payload = json!({
            "tool_name": "WebFetch",
            "tool_input": {"url": "https://example.com"},
            "tool_response": {"content": [{"type": "text", "text": "hello world"}]},
        });
        let args = web_fetch_store_args(&payload, None).expect("should handle object response");
        let content = args["content"].as_str().unwrap();
        assert!(
            content.contains("hello world"),
            "extracted text from object response"
        );
    }
}
