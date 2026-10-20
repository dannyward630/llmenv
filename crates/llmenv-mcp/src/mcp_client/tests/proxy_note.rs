use super::*;

/// Runs in a child process, which gets `HTTPS_PROXY` through `Command::env`, because `set_var`
/// is unsafe in edition 2024. A request to a closed port fails, and the error names the
/// variable (#2884).
#[tokio::test]
async fn a_failed_request_names_the_set_proxy_variable() {
    const TARGET_ENV: &str = "LLMENV_MCP_PROXY_NOTE_TARGET";
    if let Ok(target) = std::env::var(TARGET_ENV) {
        let client = McpHttpClient::with_headers(
            target,
            Duration::from_secs(5),
            &std::collections::BTreeMap::new(),
        )
        .expect("client");
        let error = client.probe().await.expect_err("a closed port must fail");
        let text = format!("{error:#}");
        assert!(text.contains("HTTPS_PROXY is set"), "{text}");
        assert!(text.contains("docs/mcp"), "{text}");
        let request = text
            .find("for MCP initialize")
            .expect("the request is named");
        assert!(request < text.find("HTTPS_PROXY").expect("note"), "{text}");
        return;
    }
    // Bind and drop a listener, so the port is closed and the connection is refused at once.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("address");
    let test_name = concat!(
        "mcp_client::tests::proxy_note::",
        "a_failed_request_names_the_set_proxy_variable"
    );
    // The child re-runs this test binary, so the path is the test binary itself.
    // nosemgrep: rust.lang.security.current-exe.current-exe
    let test_binary = std::env::current_exe().expect("test binary path");
    let mut child = std::process::Command::new(test_binary);
    // A proxy variable of the test host would add its own name to the note.
    for name in crate::proxy_note::PROXY_VARS {
        child.env_remove(name);
    }
    let output = child
        .args(["--exact", test_name, "--nocapture"])
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env(TARGET_ENV, format!("http://{closed}/mcp"))
        .output()
        .expect("spawn the child test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    // A filter that matches nothing exits 0, so require that the child ran the test.
    assert!(
        stdout.contains("1 passed"),
        "child did not run the test: {stdout}"
    );
}
