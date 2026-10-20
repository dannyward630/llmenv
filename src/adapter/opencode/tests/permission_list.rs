//! The `permissions` list that materialize writes for opencode 2.x (#2846).

use super::*;

fn rule(tool: &str, pattern: Option<&str>) -> crate::config::PermissionRule {
    crate::config::PermissionRule {
        tool: tool.into(),
        pattern: pattern.map(Into::into),
        paths: vec![],
    }
}

fn rendered(caps: crate::config::Capabilities) -> serde_json::Value {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = MergedManifest {
        capabilities: caps,
        ..Default::default()
    };
    OpencodeAdapter.materialize(&manifest, tmp.path()).unwrap();
    let raw = std::fs::read_to_string(tmp.path().join(OPENCODE_JSON_FILE)).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn rules_are_a_list_in_the_order_opencode_evaluates() {
    let mut caps = crate::config::Capabilities::default();
    caps.permissions.allow.push(rule("Bash", Some("git *")));
    caps.permissions.deny.push(rule("Bash", Some("git push *")));
    caps.permissions.ask.push(rule("Read", None));
    let doc = rendered(caps);
    assert!(
        doc.get("permission").is_none(),
        "the 1.x map is not written"
    );
    assert_eq!(
        doc["permissions"],
        serde_json::json!([
            { "action": "read", "resource": "*", "effect": "ask" },
            { "action": "shell", "resource": "git *", "effect": "allow" },
            { "action": "shell", "resource": "git push *", "effect": "deny" },
        ]),
        "the narrower deny must come after the broad allow, because the last match wins"
    );
}

#[test]
fn tools_that_opencode_2_lacks_are_dropped() {
    let mut caps = crate::config::Capabilities::default();
    caps.permissions.allow.push(rule("TodoWrite", None));
    caps.permissions.allow.push(rule("LS", None));
    caps.permissions.allow.push(rule("Task", None));
    let doc = rendered(caps);
    assert_eq!(
        doc["permissions"],
        serde_json::json!([{ "action": "subagent", "resource": "*", "effect": "allow" }])
    );
}

/// opencode 2.x takes a resource on every action, so a pattern on `webfetch` renders (it was a
/// hard error for the 1.x schema, #1328).
#[test]
fn a_pattern_on_webfetch_renders_as_a_resource() {
    let mut caps = crate::config::Capabilities::default();
    caps.permissions
        .allow
        .push(rule("WebFetch", Some("https://example.com/*")));
    let doc = rendered(caps);
    assert_eq!(
        doc["permissions"],
        serde_json::json!([
            { "action": "webfetch", "resource": "https://example.com/*", "effect": "allow" }
        ])
    );
}

/// A native 1.x key reaches the action opencode applies it to, so the order check sees the
/// structured `Bash` rule and the native `bash` rule as one action.
#[test]
fn a_native_1x_key_is_checked_as_its_2x_action() {
    let mut caps = crate::config::Capabilities::default();
    caps.permissions.deny.push(rule("Bash", Some("rm *")));
    caps.native_permissions.insert(
        "opencode".into(),
        crate::config::NativePermissionRules {
            allow: vec!["bash".into(), "task".into(), "write(src/*)".into()],
            ..Default::default()
        },
    );
    let doc = rendered(caps);
    assert_eq!(
        doc["permissions"],
        serde_json::json!([
            { "action": "edit", "resource": "src/*", "effect": "allow" },
            { "action": "shell", "resource": "*", "effect": "allow" },
            { "action": "shell", "resource": "rm *", "effect": "deny" },
            { "action": "subagent", "resource": "*", "effect": "allow" },
        ])
    );
}
