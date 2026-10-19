//! Each subagent keeps its own hook state, apart from the parent and from other subagents (#2936).

use serde_json::{Value, json};

use super::resolve_pre_tool_text;

fn config(features: Value) -> crate::config::Config {
    crate::config::Config {
        features: Some(serde_json::from_value(features).expect("test features")),
        ..Default::default()
    }
}

fn call(tool: &str, input: Value, agent: Option<&str>) -> Value {
    let mut payload = json!({ "tool_name": tool, "tool_input": input });
    if let Some(agent) = agent {
        payload["agent_id"] = json!(agent);
    }
    payload
}

fn denied(text: Option<&String>) -> bool {
    text.is_some_and(|t| t.starts_with("__DENY__"))
}

#[test]
fn a_subagent_reads_a_file_that_the_parent_already_read() {
    let dir = tempfile::tempdir().expect("test");
    let file = dir.path().join("f.txt");
    std::fs::write(&file, "body").expect("test");
    let cfg = config(json!({ "read_once": { "enabled": true, "mode": "deny" } }));
    let read = |agent: Option<&str>| {
        let payload = call("Read", json!({ "file_path": file }), agent);
        resolve_pre_tool_text(&payload, Some("s1"), &cfg, false, dir.path())
    };
    assert!(!denied(read(None).as_ref()));
    assert!(
        !denied(read(Some("a1")).as_ref()),
        "the subagent never saw the file, so its first read must pass"
    );
    assert!(
        denied(read(Some("a1")).as_ref()),
        "the subagent's own second read is still a re-read"
    );
}

#[test]
fn a_subagent_must_read_a_file_itself_before_it_writes_the_file() {
    let dir = tempfile::tempdir().expect("test");
    let file = dir.path().join("f.txt");
    std::fs::write(&file, "body").expect("test");
    let cfg = config(json!({ "slippage": { "enabled": true } }));
    let run = |tool: &str, agent: Option<&str>| {
        let payload = call(tool, json!({ "file_path": file }), agent);
        resolve_pre_tool_text(&payload, Some("s1"), &cfg, false, dir.path())
    };
    assert!(!denied(run("Read", None).as_ref()));
    assert!(
        denied(run("Write", Some("a1")).as_ref()),
        "the parent's read is not in the subagent's context"
    );
    assert!(!denied(run("Write", None).as_ref()));
}

#[test]
fn the_commit_gate_denies_the_first_commit_of_each_agent() {
    let dir = tempfile::tempdir().expect("test");
    let cfg = config(json!({ "task_tracker": { "enabled": true } }));
    let commit = |agent: Option<&str>| {
        let payload = call("Bash", json!({ "command": "git commit -m x" }), agent);
        resolve_pre_tool_text(&payload, Some("s1"), &cfg, true, dir.path())
    };
    assert!(denied(commit(None).as_ref()), "the first commit is denied");
    assert!(!denied(commit(None).as_ref()), "the retry passes");
    assert!(
        denied(commit(Some("a1")).as_ref()),
        "the parent's retry must not let the subagent's first commit through"
    );
}
