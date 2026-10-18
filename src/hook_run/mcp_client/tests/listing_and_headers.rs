use super::*;

#[tokio::test]
async fn list_tools_reads_an_sse_reply() {
    let server = server_with_session(None).await;
    let payload = serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"s"}]}});
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/list"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!("event: message\ndata: {payload}\n\n")),
        )
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    assert_eq!(client.list_tools().await.unwrap()[0].name, "s");
}

#[tokio::test]
async fn a_client_without_headers_does_not_follow_a_redirect() {
    let target = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(init_ok("s", None))
        .expect(0)
        .mount(&target)
        .await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", target.uri()))
        .mount(&server)
        .await;
    let client = McpHttpClient::new(server.uri(), Duration::from_secs(2)).expect("client");
    assert!(client.probe().await.is_err());
}

#[tokio::test]
async fn with_headers_does_not_follow_a_redirect() {
    let target = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(init_ok("s", None))
        .expect(0)
        .mount(&target)
        .await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", target.uri()))
        .mount(&server)
        .await;
    let headers = std::collections::BTreeMap::from([("x-api-key".to_string(), "k".to_string())]);
    let client = McpHttpClient::with_headers(server.uri(), Duration::from_secs(2), &headers)
        .expect("client");
    assert!(client.probe().await.is_err());
}

#[tokio::test]
async fn list_tools_joins_the_pages_in_order() {
    let server = server_with_session(None).await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/list"))
        .and(Cursor(None))
        .respond_with(tools_page(&["a", "b"], Some("p2")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/list"))
        .and(Cursor(Some("p2")))
        .respond_with(tools_page(&["c"], None))
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let tools = client.list_tools().await.expect("tools");
    let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert_eq!(tools[2].description.as_deref(), Some("about c"));
}

#[tokio::test]
async fn list_tools_with_one_page_makes_one_request() {
    let server = server_with_session(None).await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/list"))
        .respond_with(tools_page(&["only"], None))
        .expect(1)
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    assert_eq!(client.list_tools().await.expect("tools").len(), 1);
}

#[tokio::test]
async fn list_tools_stops_at_the_page_cap_and_names_it() {
    let server = server_with_session(None).await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/list"))
        .respond_with(tools_page(&["t"], Some("again")))
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let err = client.list_tools().await.expect_err("cap").to_string();
    assert!(err.contains(&MAX_TOOL_PAGES.to_string()), "{err}");
}

#[tokio::test]
async fn list_tools_reinitializes_once_after_a_stale_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("initialize"))
        .respond_with(init_ok("sess-stale", None))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("initialize"))
        .respond_with(init_ok("sess-fresh", None))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/list"))
        .and(wiremock::matchers::header("mcp-session-id", "sess-stale"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/list"))
        .and(wiremock::matchers::header("mcp-session-id", "sess-fresh"))
        .respond_with(tools_page(&["ok"], None))
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    assert_eq!(client.list_tools().await.expect("tools")[0].name, "ok");
}

#[tokio::test]
async fn list_tools_fails_on_a_reply_without_a_tools_array() {
    let server = server_with_session(None).await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("tools/list"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": {} })),
        )
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let err = client.list_tools().await.expect_err("no tools").to_string();
    assert!(err.contains("result.tools"), "{err}");
}

#[tokio::test]
async fn with_headers_sends_the_configured_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::header("x-api-key", "secret"))
        .respond_with(init_ok("s1", None))
        .expect(2) // the initialize and its acknowledgement both carry the header
        .mount(&server)
        .await;
    let headers =
        std::collections::BTreeMap::from([("x-api-key".to_string(), "secret".to_string())]);
    let client = McpHttpClient::with_headers(server.uri(), Duration::from_secs(2), &headers)
        .expect("client");
    client.probe().await.expect("probe");
}

/// The error text that `with_headers` returns for `headers`.
fn header_error(headers: &std::collections::BTreeMap<String, String>) -> String {
    McpHttpClient::with_headers("http://127.0.0.1:9".into(), Duration::from_secs(1), headers)
        .expect_err("invalid header")
        .to_string()
}

#[test]
fn with_headers_rejects_an_invalid_header_name_and_value() {
    let bad_name = std::collections::BTreeMap::from([("bad name".to_string(), "v".to_string())]);
    let name_err = header_error(&bad_name);
    assert!(
        name_err.contains("MCP header name 'bad name' is not valid"),
        "{name_err}"
    );
    let bad_value =
        std::collections::BTreeMap::from([("x-ok".to_string(), "line\nbreak".to_string())]);
    let value_err = header_error(&bad_value);
    assert!(
        value_err.contains("MCP header 'x-ok' has a value that is not valid"),
        "{value_err}"
    );
}

#[test]
fn with_headers_rejects_keys_that_differ_only_by_case() {
    let headers = std::collections::BTreeMap::from([
        ("X-Api-Key".to_string(), "a".to_string()),
        ("x-api-key".to_string(), "b".to_string()),
    ]);
    let err = header_error(&headers);
    assert!(err.contains("is set more than once"), "{err}");
}

/// Runs in a child process. The child gets the proxy variables through `Command::env`, so the
/// parent process env stays untouched, because `set_var` is unsafe in edition 2024.
#[tokio::test]
async fn http_proxy_env_cannot_carry_an_mcp_request() {
    const TARGET_ENV: &str = "LLMENV_MCP_PROXY_TEST_TARGET";
    if let Ok(target) = std::env::var(TARGET_ENV) {
        let client = McpHttpClient::with_headers(
            target,
            Duration::from_secs(5),
            &std::collections::BTreeMap::new(),
        )
        .expect("client");
        client
            .probe()
            .await
            .expect("the request must reach the target, not the proxy");
        return;
    }
    let target = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(init_ok("s", None))
        .mount(&target)
        .await;
    let proxy = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(418))
        .mount(&proxy)
        .await;
    let test_name = concat!(
        "hook_run::mcp_client::tests::listing_and_headers::",
        "http_proxy_env_cannot_carry_an_mcp_request"
    );
    // The child re-runs this test binary, so the path is the test binary itself. nosemgrep: rust.lang.security.current-exe.current-exe
    let test_binary = std::env::current_exe().expect("test binary path");
    let output = std::process::Command::new(test_binary)
        .args(["--exact", test_name, "--nocapture"])
        .env("HTTP_PROXY", proxy.uri())
        .env("HTTPS_PROXY", proxy.uri())
        .env("ALL_PROXY", proxy.uri())
        .env(TARGET_ENV, target.uri())
        .output()
        .expect("spawn the child test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "a proxy carried the MCP request: {stdout}"
    );
    // A filter that matches nothing exits 0, so require that the child ran the test.
    assert!(
        stdout.contains("1 passed"),
        "child did not run the test: {stdout}"
    );
}
