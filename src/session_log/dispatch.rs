//! Issues the two transcript MCP calls through the shared `McpHttpClient`.
//! Callers run these inside a current-thread tokio runtime (see `hook_run`).

use serde_json::Value;

use crate::hook_run::mcp_client::McpHttpClient;
use crate::session_log::event::SessionLogEvent;
use crate::session_log::transcript::{
    RECORD_TOOL, SHOW_TOOL, START_TOOL, record_args, show_session_args, start_session_args,
};

/// Start a transcript session; returns its id (the tool's text result, trimmed).
///
/// # Errors
/// Any `call_tool` failure, an empty id, or a non-ASCII id (#509 item 1:
/// defense-in-depth — the id is persisted to `transcript-sessions.json` and
/// later passed as a CLI/process argument elsewhere, so a well-formed id
/// matters even though ICM is a trusted boundary today). Every valid session
/// id (UUID, ULID) is pure ASCII, so rejecting anything else — not just
/// `is_whitespace()`/`is_control()`, which miss Unicode formatting characters
/// like zero-width space or RTL override — closes the gap with no false
/// positives.
pub(crate) async fn start_session(
    client: &McpHttpClient,
    agent: &str,
    project: Option<&str>,
    metadata: &Value,
) -> anyhow::Result<String> {
    let text = client
        .call_tool(START_TOOL, start_session_args(agent, project, metadata))
        .await?;
    let raw = text.trim();
    // ICM returns the session id as a JSON object like `{"session_id":"01KXE..."}`,
    // not as a bare id. Parse it; fall back to the raw text for forward compat
    // with backend changes.
    let id = serde_json::from_str::<Value>(raw)
        .inspect_err(|e| {
            tracing::warn!("failed to parse ICM session id response: {e}");
        })
        .ok()
        .and_then(|v| {
            v.get("session_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| raw.to_string());
    if id.is_empty() {
        anyhow::bail!("{START_TOOL} returned an empty session id");
    }
    if !id.is_ascii() || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        anyhow::bail!("{START_TOOL} returned a malformed session id: {id:?}");
    }
    Ok(id)
}

/// Text in ICM's JSON-RPC error when a session id is unknown. ICM has no typed
/// not-found code, so this text is the only signal.
const SESSION_GONE_MARKER: &str = "session not found";

/// Whether ICM still holds a recorded transcript session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Liveness {
    /// ICM returned the session.
    Live,
    /// ICM answered that the session does not exist.
    Gone,
}

/// Ask ICM whether `session_id` still exists.
///
/// A recorded session id is a cache, not proof of liveness (#1090). ICM may
/// have pruned or restarted since it was recorded.
///
/// # Errors
/// A transport failure, or any reply that is not "session not found". ICM being
/// unreachable says nothing about the session, so the caller MUST keep its link
/// then (#2667).
pub(crate) async fn verify_session(
    client: &McpHttpClient,
    session_id: &str,
) -> anyhow::Result<Liveness> {
    match client
        .call_tool(SHOW_TOOL, show_session_args(session_id))
        .await
    {
        Ok(_) => Ok(Liveness::Live),
        Err(e) if format!("{e:#}").contains(SESSION_GONE_MARKER) => Ok(Liveness::Gone),
        Err(e) => Err(e),
    }
}

/// Record one event into `session_id`.
///
/// # Errors
/// Any `call_tool` failure.
pub(crate) async fn record(
    client: &McpHttpClient,
    session_id: &str,
    ev: &SessionLogEvent,
) -> anyhow::Result<()> {
    client
        .call_tool(RECORD_TOOL, record_args(session_id, ev))
        .await?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::session_log::event::{EventKind, EventScope, SessionLogEvent};
    use std::time::Duration;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn text_result(text: &str) -> serde_json::Value {
        serde_json::json!({"jsonrpc":"2.0","id":1,
            "result":{"content":[{"type":"text","text":text}]}})
    }

    #[tokio::test]
    async fn start_session_parses_returned_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(text_result("sess-42")))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        let id = start_session(
            &client,
            "claude_code",
            Some("llmenv"),
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        assert_eq!(id, "sess-42");
    }

    #[tokio::test]
    async fn start_session_extracts_session_id_from_json_object() {
        // ICM returns `{"session_id":"..."}`, not a bare ULID.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(text_result(
                r#"{"session_id":"01KXE1FNZCF1A0EAHK5X207RBW"}"#,
            )))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        let id = start_session(
            &client,
            "claude_code",
            Some("llmenv"),
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        assert_eq!(id, "01KXE1FNZCF1A0EAHK5X207RBW");
    }

    #[tokio::test]
    async fn start_session_rejects_id_with_whitespace() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(text_result("sess 42")))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        let err = start_session(
            &client,
            "claude_code",
            Some("llmenv"),
            &serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("malformed session id"));
    }

    #[tokio::test]
    async fn start_session_rejects_id_with_control_character() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(text_result("sess\x0042")))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        let err = start_session(
            &client,
            "claude_code",
            Some("llmenv"),
            &serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("malformed session id"));
    }

    #[tokio::test]
    async fn start_session_rejects_non_ascii_id() {
        // Zero-width space (U+200B): not caught by is_whitespace()/is_control(),
        // but is_ascii() rejects it.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(text_result("sess\u{200B}42")))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        let err = start_session(
            &client,
            "claude_code",
            Some("llmenv"),
            &serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("malformed session id"));
    }

    #[tokio::test]
    async fn record_posts_without_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(text_result("ok")))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        let ev = SessionLogEvent {
            ts: "t".into(),
            kind: EventKind::Prompt,
            scope: EventScope::AgentSession,
            role: "user".into(),
            tool_name: None,
            tokens: None,
            level: None,
            content: "hi".into(),
            fields: serde_json::json!({}),
            trace_fields: None,
        };
        record(&client, "sess-42", &ev).await.unwrap();
    }

    #[tokio::test]
    async fn verify_session_succeeds_when_icm_returns_the_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(text_result("[]")))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        assert_eq!(
            verify_session(&client, "sess-42").await.unwrap(),
            Liveness::Live
        );
    }

    #[tokio::test]
    async fn verify_session_reports_gone_when_icm_says_session_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc":"2.0","id":1,
                "error":{"code":-32602,"message":"session not found"}
            })))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        assert_eq!(
            verify_session(&client, "stale-session").await.unwrap(),
            Liveness::Gone
        );
    }

    #[tokio::test]
    async fn verify_session_errors_when_icm_is_unreachable() {
        // A transport failure must not read as "gone": the caller keeps the link.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        assert!(verify_session(&client, "sess-42").await.is_err());
    }
}
