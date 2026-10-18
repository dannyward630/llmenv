use super::*;

#[tokio::test]
async fn probe_succeeds_when_server_answers_initialize() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("initialize"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("mcp-session-id", "s1")
                .set_body_json(serde_json::json!({"jsonrpc": "2.0", "id": 0, "result": {}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(JsonRpcMethod("notifications/initialized"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;

    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    client.probe().await.expect("probe ok");
}

#[tokio::test]
async fn probe_errors_on_http_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let err = client.probe().await.expect_err("500 must fail the probe");
    assert!(err.to_string().contains("500"), "got: {err}");
}

#[tokio::test]
async fn probe_errors_when_server_accepts_but_never_answers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;

    let client =
        McpHttpClient::test_new(server.uri(), Duration::from_millis(200)).expect("valid URL");
    assert!(client.probe().await.is_err(), "a wedged server must fail");
}

#[tokio::test]
async fn initialize_info_returns_the_instructions() {
    let server = server_with_session(Some("be careful")).await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    let info = client.initialize_info().await.expect("info");
    assert_eq!(info.instructions.as_deref(), Some("be careful"));
}

#[tokio::test]
async fn initialize_info_is_empty_when_the_server_sends_no_instructions() {
    let server = server_with_session(None).await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    assert_eq!(
        client.initialize_info().await.expect("info"),
        InitializeInfo::default()
    );
}

#[test]
fn initialize_info_reads_json_and_sse_bodies() {
    let json = r#"{"result":{"instructions":"a"}}"#;
    assert_eq!(
        parse_initialize_info(json).unwrap().instructions.as_deref(),
        Some("a")
    );
    let sse = format!("event: message\ndata: {json}\n\n");
    assert_eq!(
        parse_initialize_info(&sse).unwrap().instructions.as_deref(),
        Some("a")
    );
    // A notification event ahead of the reply is skipped.
    let two = format!("data: {{\"method\":\"notifications/x\"}}\n\ndata: {json}\n\n");
    assert_eq!(
        parse_initialize_info(&two).unwrap().instructions.as_deref(),
        Some("a")
    );
    let none = r#"{"result":{}}"#;
    assert_eq!(
        parse_initialize_info(none).unwrap(),
        InitializeInfo::default()
    );
}

#[test]
fn initialize_info_fails_loudly_on_a_reply_it_cannot_read() {
    for body in [
        "not json",
        "",
        r#"{"id":0}"#,
        r#"{"result":{"instructions":5}}"#,
        r#"{"error":{"message":"no"}}"#,
    ] {
        assert!(parse_initialize_info(body).is_err(), "{body}");
    }
}

#[test]
fn a_tools_page_rejects_malformed_entries() {
    let ok = serde_json::json!({"tools":[{"name":"a"},{"name":"b","description":null}],"nextCursor":"n"});
    let (tools, next) = parse_tools_page(&ok).unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(next.as_deref(), Some("n"));
    for bad in [
        serde_json::json!({}),
        serde_json::json!({"tools":[{"description":"x"}]}),
        serde_json::json!({"tools":[{"name":3}]}),
        serde_json::json!({"tools":[{"name":"a","description":4}]}),
    ] {
        assert!(parse_tools_page(&bad).is_err(), "{bad}");
    }
    let empty_cursor = serde_json::json!({"tools":[],"nextCursor":""});
    assert_eq!(parse_tools_page(&empty_cursor).unwrap().1, None);
}

#[test]
fn error_text_hides_the_url_query_and_credentials() {
    let client = McpHttpClient::test_new(
        "http://user:pw@127.0.0.1:9/mcp?token=secret#f".into(),
        Duration::from_secs(1),
    )
    .unwrap();
    let shown = client.display_url();
    assert!(!shown.contains("secret") && !shown.contains("pw") && !shown.contains("user"));
    assert!(shown.contains("127.0.0.1:9/mcp"));
}

proptest::proptest! {
    #[test]
    fn an_initialize_reply_reads_the_same_as_json_or_sse(text in "[ -~é]{0,200}") {
        let json = serde_json::json!({"result": {"instructions": text}}).to_string();
        let sse = format!("event: message\ndata: {json}\n\n");
        let plain = parse_initialize_info(&json).unwrap();
        proptest::prop_assert_eq!(&plain, &parse_initialize_info(&sse).unwrap());
        proptest::prop_assert_eq!(plain.instructions, Some(text));
    }

    #[test]
    fn no_body_makes_the_reply_parser_panic(body in "\\PC{0,200}") {
        let _ = parse_initialize_info(&body);
        let _ = parse_rpc_body(&body);
    }
}

#[tokio::test]
async fn initialize_info_fails_on_an_unreadable_reply_from_a_server() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("garbage"))
        .mount(&server)
        .await;
    let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).expect("valid URL");
    assert!(client.initialize_info().await.is_err());
}
