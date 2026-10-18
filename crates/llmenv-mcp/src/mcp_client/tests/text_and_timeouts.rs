use super::*;

#[test]
fn run_with_timeout_errors_when_exceeded() {
    // The closure sleeps far longer than the timeout, so the race is
    // decided by the sleep vs. timeout durations rather than by real DNS
    // resolution latency, which varies with scheduling and resolver
    // caching and made this test flaky under slow (e.g. coverage) runs.
    let result = run_with_timeout(
        || {
            thread::sleep(Duration::from_millis(50));
            42
        },
        Duration::from_millis(1),
    );
    assert_eq!(result, Err(mpsc::RecvTimeoutError::Timeout));
}

#[test]
fn resolve_with_timeout_resolves_within_budget() {
    let addrs = resolve_with_timeout("localhost", 80, Duration::from_secs(5))
        .expect("localhost must resolve well within 5s");
    assert!(!addrs.is_empty());
}

#[test]
fn extract_text_handles_missing_result() {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1
    });
    assert_eq!(extract_text(&body), None);
}

#[test]
fn extract_text_handles_missing_content() {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "other_field": "data" }
    });
    assert_eq!(extract_text(&body), None);
}

#[test]
fn extract_text_handles_non_array_content() {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "content": "not an array" }
    });
    assert_eq!(extract_text(&body), None);
}

#[test]
fn extract_text_concatenates_multiple_text_items() {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "content": [
                { "type": "text", "text": "first" },
                { "type": "text", "text": "second" },
                { "type": "text", "text": "third" }
            ]
        }
    });
    assert_eq!(
        extract_text(&body),
        Some("first\nsecond\nthird".to_string())
    );
}

#[test]
fn extract_text_skips_non_text_items() {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "content": [
                { "type": "text", "text": "a" },
                { "type": "image", "url": "https://example.com/img.png" },
                { "type": "text", "text": "b" }
            ]
        }
    });
    assert_eq!(extract_text(&body), Some("a\nb".to_string()));
}

#[test]
fn extract_text_handles_missing_text_field() {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "content": [
                { "type": "text" },
                { "type": "text", "text": "valid" }
            ]
        }
    });
    assert_eq!(extract_text(&body), Some("valid".to_string()));
}

#[test]
fn extract_text_handles_empty_content_array() {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": { "content": [] }
    });
    assert_eq!(extract_text(&body), Some(String::new()));
}
