//! Tests of the per-subagent counter (#2901).

use super::tests::{bash_payload, config};
use super::*;
use serde_json::json;
use tempfile::TempDir;

fn agent_payload(cmd: &str, agent_id: &str) -> serde_json::Value {
    json!({ "tool_name": "Bash", "tool_input": { "command": cmd }, "agent_id": agent_id })
}

#[test]
fn parallel_subagents_with_one_identical_call_each_get_no_warning() {
    let dir = TempDir::new().expect("test");
    for agent in ["a1", "a2", "a3"] {
        let out = handle_pre_tool_use(
            &agent_payload("ls", agent),
            Some("s1"),
            &config(3),
            dir.path(),
        );
        assert!(out.is_empty(), "agent {agent} made one call and got: {out}");
    }
}

#[test]
fn one_subagent_that_repeats_a_call_still_gets_the_warning() {
    let dir = TempDir::new().expect("test");
    let payload = agent_payload("ls", "a1");
    let outs: Vec<String> = (0..3)
        .map(|_| handle_pre_tool_use(&payload, Some("s1"), &config(3), dir.path()))
        .collect();
    assert!(outs[0].is_empty() && outs[1].is_empty());
    assert!(!outs[2].is_empty(), "the third identical call must warn");
}

#[test]
fn a_subagent_call_does_not_reset_the_main_agent_streak() {
    let dir = TempDir::new().expect("test");
    let main = bash_payload("ls");
    handle_pre_tool_use(&main, Some("s1"), &config(3), dir.path());
    handle_pre_tool_use(&main, Some("s1"), &config(3), dir.path());
    handle_pre_tool_use(
        &agent_payload("pwd", "a1"),
        Some("s1"),
        &config(3),
        dir.path(),
    );
    let out = handle_pre_tool_use(&main, Some("s1"), &config(3), dir.path());
    assert!(
        !out.is_empty(),
        "the main agent's third identical call must warn"
    );
}

#[test]
fn an_agent_id_with_path_parts_gets_its_own_counter_inside_the_state_dir() {
    let dir = TempDir::new().expect("test");
    let outs: Vec<String> = (0..2)
        .map(|_| {
            handle_pre_tool_use(
                &agent_payload("ls", "../x"),
                Some("s1"),
                &config(2),
                dir.path(),
            )
        })
        .collect();
    assert!(outs[0].is_empty());
    assert!(!outs[1].is_empty(), "the second identical call must warn");
    assert!(!dir.path().join("x.json").exists());
    assert!(!repeat_detect_state_dir(dir.path()).join("s1.json").exists());
}

#[test]
fn a_non_string_agent_id_still_keeps_its_own_counter() {
    let dir = TempDir::new().expect("test");
    let numeric = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "agent_id": 7 });
    handle_pre_tool_use(&bash_payload("ls"), Some("s1"), &config(2), dir.path());
    let out = handle_pre_tool_use(&numeric, Some("s1"), &config(2), dir.path());
    assert!(
        out.is_empty(),
        "the subagent must not add to the main agent's streak: {out}"
    );
}

proptest::proptest! {
    #[test]
    fn two_different_agents_never_share_a_state_key(
        s1 in "[a-z0-9.]{1,6}", a1 in "[a-z0-9.]{1,6}",
        s2 in "[a-z0-9.]{1,6}", a2 in proptest::option::of("[a-z0-9.]{1,6}"),
    ) {
        let payload = |a: Option<&str>| match a {
            Some(a) => json!({ "agent_id": a }),
            None => json!({}),
        };
        let k1 = state_key(&s1, &payload(Some(&a1)));
        let k2 = state_key(&s2, &payload(a2.as_deref()));
        if (s1.as_str(), Some(a1.as_str())) != (s2.as_str(), a2.as_deref()) {
            proptest::prop_assert_ne!(&k1, &k2);
        }
        proptest::prop_assert!(crate::paths::is_valid_short_name(&k1) || s1.starts_with('.'));
    }
}
