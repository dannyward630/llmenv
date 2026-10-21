use std::path::Path;

use super::{render_options, render_permission_lists};
use crate::config::{PermissionRule, Permissions};

fn rule(tool: &str) -> PermissionRule {
    PermissionRule {
        tool: tool.into(),
        pattern: None,
        paths: vec![],
    }
}

#[test]
fn unscoped_deny_becomes_disabled_tool() {
    let perms = Permissions {
        deny: vec![rule("Bash")],
        ..Default::default()
    };
    let (allowed, disabled) = render_permission_lists(&perms, None);
    assert!(allowed.is_empty());
    assert_eq!(disabled, vec!["bash".to_string()]);
}

#[test]
fn pattern_scoped_deny_is_not_rendered_as_disabled_tool() {
    let mut scoped = rule("Bash");
    scoped.pattern = Some("rm *".into());
    let perms = Permissions {
        deny: vec![scoped],
        ..Default::default()
    };
    let (_, disabled) = render_permission_lists(&perms, None);
    assert!(disabled.is_empty());
}

#[test]
fn deny_withholds_allow_of_the_same_tool() {
    let perms = Permissions {
        allow: vec![rule("Bash"), rule("Read")],
        deny: vec![rule("Bash")],
        ..Default::default()
    };
    let (allowed, disabled) = render_permission_lists(&perms, None);
    assert_eq!(allowed, vec!["view".to_string()]);
    assert_eq!(disabled, vec!["bash".to_string()]);
}

#[test]
fn task_and_ask_user_question_allow_map_to_crush_tools() {
    let perms = Permissions {
        allow: vec![rule("Task"), rule("AskUserQuestion")],
        ..Default::default()
    };
    let (allowed, _) = render_permission_lists(&perms, None);
    assert!(allowed.contains(&"agent".to_string()), "{allowed:?}");
    assert!(allowed.contains(&"question".to_string()), "{allowed:?}");
}

#[test]
fn render_options_is_none_when_nothing_applies() {
    assert!(
        render_options(Path::new("/x"), false, &[])
            .unwrap()
            .is_none()
    );
}

#[test]
fn render_options_carries_skills_and_disabled_tools() {
    let v = render_options(Path::new("/x"), true, &["bash".into()])
        .unwrap()
        .unwrap();
    assert_eq!(v["skills_paths"][0], "/x/skills");
    assert_eq!(v["disabled_tools"][0], "bash");
}

#[test]
fn render_options_omits_skills_key_without_skills() {
    let v = render_options(Path::new("/x"), false, &["bash".into()])
        .unwrap()
        .unwrap();
    assert!(v.get("skills_paths").is_none());
    assert_eq!(v["disabled_tools"][0], "bash");
}
