//! The paths where a nudge or the commit gate cannot do its work. Each one is named to the agent
//! (#2536).

use super::*;

fn notices() -> String {
    crate::agent_notice::current().unwrap_or_default()
}

#[test]
fn a_bash_call_without_a_string_command_is_denied_and_named() {
    let dir = TempDir::new().unwrap();
    let tracker = TaskTracker::default();
    for input in [
        serde_json::json!({}),
        serde_json::json!({ "command": 42 }),
        serde_json::json!({ "command": null }),
    ] {
        let payload = serde_json::json!({ "tool_name": "Bash", "tool_input": input });
        let logs = crate::test_log_capture::capture_logs(|| {
            let out = handle_pre_tool_use(&tracker, &payload, Some("s1"), dir.path());
            assert!(
                out.starts_with("__DENY__:") && out.contains("tool_input.command"),
                "{input}: {out}"
            );
        });
        assert!(
            logs.contains("tool_input.command"),
            "logged at error: {logs}"
        );
    }
}

#[test]
fn a_nudge_with_no_session_id_is_named_to_the_agent() {
    let dir = TempDir::new().unwrap();
    let tracker = TaskTracker::default();
    let edit = serde_json::json!({ "tool_name": "Edit" });
    assert_eq!(handle_post_tool_use(&tracker, &edit, None, dir.path()), "");
    assert!(notices().contains("session id"), "{}", notices());
}

#[test]
fn a_read_only_call_with_no_session_id_adds_no_notice() {
    let dir = TempDir::new().unwrap();
    let tracker = TaskTracker::default();
    let read = serde_json::json!({ "tool_name": "Read" });
    handle_post_tool_use(&tracker, &read, None, dir.path());
    assert_eq!(crate::agent_notice::current(), None);
}

#[cfg(unix)]
#[test]
fn a_nudge_state_that_cannot_be_saved_is_named_to_the_agent() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    // A read-only state directory: the state reads as new, and the save cannot create its folder.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let tracker = TaskTracker::default();
    let edit = serde_json::json!({ "tool_name": "Edit" });
    handle_post_tool_use(&tracker, &edit, Some("s1"), dir.path());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        notices().contains("cannot be saved"),
        "the lost update must be named: {}",
        notices()
    );
}

#[test]
fn a_corrupt_state_file_is_rewritten_so_the_reset_is_named_once() {
    let dir = TempDir::new().unwrap();
    let path = state_path(dir.path(), "s1").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{not json").unwrap();
    let tracker = TaskTracker::default();
    let read = serde_json::json!({ "tool_name": "Read" });
    handle_post_tool_use(&tracker, &read, Some("s1"), dir.path());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        serde_json::from_str::<NudgeState>(&text).is_ok(),
        "the reset must reach the file: {text}"
    );
}

#[test]
fn the_commit_gate_rewrites_a_corrupt_state_file() {
    let dir = TempDir::new().unwrap();
    let path = state_path(dir.path(), "s1").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{not json").unwrap();
    let tracker = TaskTracker::default();
    let session = open_session(dir.path());
    let slug = add(dir.path(), &session, "Step one");
    start_task(dir.path(), &slug, false).unwrap();
    assert_eq!(
        handle_pre_tool_use(&tracker, &bash("git commit"), Some("s1"), dir.path()),
        ""
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(serde_json::from_str::<NudgeState>(&text).is_ok(), "{text}");
}
