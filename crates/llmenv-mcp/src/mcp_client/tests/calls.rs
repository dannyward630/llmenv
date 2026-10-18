use super::*;

#[test]
fn emit_mcp_call_trace_never_panics_without_env_var() {
    emit_mcp_call_trace("icm_memory_recall", Duration::from_micros(2340));
}

#[tokio::test]
async fn call_tool_returns_an_error_when_the_tool_reports_is_error() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "isError": true,
            "content": [{ "type": "text", "text": "database is locked" }]
        }
    });
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let err = client
        .call_tool("icm_memory_store", serde_json::json!({}))
        .await
        .expect_err("a tool error must not look like success")
        .to_string();
    assert!(err.contains("database is locked"), "{err}");
    assert!(err.contains("icm_memory_store"), "{err}");
}

#[tokio::test]
async fn call_tool_treats_is_error_false_as_success() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "isError": false, "content": [{ "type": "text", "text": "ok" }] }
    });
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let text = client
        .call_tool("icm_memory_store", serde_json::json!({}))
        .await
        .expect("isError false is success");
    assert_eq!(text, "ok");
}

#[tokio::test]
async fn call_tool_returns_text_content() {
    let _guard = port_guard();
    let server = MockServer::start().await;
    // MCP tools/call response: result.content[0].text
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "content": [{ "type": "text", "text": "wake-up pack" }]
        }
    });
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let text = client
        .call_tool("icm_wake_up", serde_json::json!({}))
        .await
        .expect("call_tool ok");
    assert_eq!(text, "wake-up pack");
}

#[tokio::test]
async fn probe_ignores_a_cached_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("initialize"))
        .respond_with(ResponseTemplate::new(200).insert_header("mcp-session-id", "s1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    client.probe().await.expect("first probe ok");
    assert!(
        client.probe().await.is_err(),
        "second probe must re-ask the server"
    );
}

#[tokio::test]
async fn call_tool_negotiates_session_and_sends_it_on_tools_call() {
    // Regression test: a real MCP Streamable HTTP server (mcp-proxy fronting
    // icm serve) hands out an Mcp-Session-Id on initialize and rejects
    // tools/call with "Bad Request: Missing session ID" without it. Before
    // the initialize handshake was added, every real call failed this way —
    // masked in practice by the (also buggy) SSRF loopback block, which
    // rejected the URL before any request was ever sent.
    let _guard = port_guard();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(JsonRpcMethod("initialize"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("mcp-session-id", "test-session-abc")
                .set_body_json(serde_json::json!({
                    "jsonrpc": "2.0", "id": 0, "result": { "protocolVersion": "2025-06-18" }
                })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(JsonRpcMethod("notifications/initialized"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(JsonRpcMethod("tools/call"))
        .and(wiremock::matchers::header(
            "mcp-session-id",
            "test-session-abc",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "content": [{ "type": "text", "text": "recalled" }] }
        })))
        .mount(&server)
        .await;
    // Without the "mcp-session-id" header requirement above, a tools/call
    // missing the header would 404 (no matcher) — the real server's 400
    // "Missing session ID" — surfacing the same failure mode.

    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let text = client
        .call_tool("icm_memory_recall", serde_json::json!({}))
        .await
        .expect("call_tool ok — session must be negotiated automatically");
    assert_eq!(text, "recalled");
}

/// #1094: the server rejecting a cached `Mcp-Session-Id` (session expired,
/// or the server restarted) must be recovered from within the same call —
/// clear the cache, re-initialize, retry — rather than replaying the dead
/// id on every subsequent call until `llmenv` is restarted.
#[tokio::test]
async fn call_tool_recovers_from_stale_session_after_one_retry() {
    let init_response = |sid: &str| {
        ResponseTemplate::new(200)
            .insert_header("mcp-session-id", sid)
            .set_body_json(serde_json::json!({
                "jsonrpc": "2.0", "id": 0, "result": { "protocolVersion": "2025-06-18" }
            }))
    };
    let _guard = port_guard();
    let server = MockServer::start().await;
    // First initialize hands out the session the cache starts with.
    Mock::given(method("POST"))
        .and(JsonRpcMethod("initialize"))
        .respond_with(init_response("sess-stale"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    // Re-initialize after the cache is cleared hands out a fresh one.
    Mock::given(method("POST"))
        .and(JsonRpcMethod("initialize"))
        .respond_with(init_response("sess-fresh"))
        .mount(&server)
        .await;
    // The stale id is rejected — mcp-proxy's own "session not found" shape.
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/call"))
        .and(wiremock::matchers::header("mcp-session-id", "sess-stale"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    // The re-initialized id succeeds.
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/call"))
        .and(wiremock::matchers::header("mcp-session-id", "sess-fresh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "content": [{ "type": "text", "text": "recovered" }] }
        })))
        .mount(&server)
        .await;

    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let text = client
        .call_tool("icm_memory_recall", serde_json::json!({}))
        .await
        .expect("call_tool must recover after one retry");
    assert_eq!(text, "recovered");
}

/// A second stale-session rejection (even after the retry re-initialized)
/// is a real error, not retried again — otherwise a server that's
/// genuinely down would loop forever instead of surfacing a failure.
#[tokio::test]
async fn call_tool_gives_up_after_second_stale_session_rejection() {
    let _guard = port_guard();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("initialize"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("mcp-session-id", "sess-always-stale")
                .set_body_json(serde_json::json!({
                    "jsonrpc": "2.0", "id": 0, "result": { "protocolVersion": "2025-06-18" }
                })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/call"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let err = client
        .call_tool("icm_memory_recall", serde_json::json!({}))
        .await
        .expect_err("a second rejection must not be retried again");
    assert!(err.to_string().contains("404"));

    let requests = server.received_requests().await.expect("received requests");
    let tools_calls = requests
        .iter()
        .filter(|r| wiremock::Match::matches(&JsonRpcMethod("tools/call"), r))
        .count();
    assert_eq!(
        tools_calls, 2,
        "exactly one retry: the original attempt plus one re-initialized retry"
    );
}

#[tokio::test]
async fn call_tool_errors_on_unreachable() {
    // Valid public IP that should reject (no listening service)
    let client = McpHttpClient::new("https://8.8.8.8:0".to_string(), Duration::from_millis(200))
        .expect("valid URL");
    let result = client.call_tool("icm_wake_up", serde_json::json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn call_tool_errors_on_jsonrpc_error() {
    let _guard = port_guard();
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": { "code": -32000, "message": "boom" }
    });
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let result = client.call_tool("icm_wake_up", serde_json::json!({})).await;
    assert!(result.is_err());
}
