//! The hook table that materialize writes to `plugin/llmenv-hooks.json` (#2840).

use super::*;

#[test]
fn materialize_hook_unsupported_event_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let mut manifest = MergedManifest::default();
    manifest.capabilities.hooks.push(crate::config::Hook {
        event: "Notification".into(),
        matcher: None,
        handler: crate::config::HookHandler {
            kind: crate::config::HookHandlerKind::Command,
            command: Some("echo n".into()),
            tool: None,
        },
        bundle_origin: None,
    });
    OpencodeAdapter.materialize(&manifest, tmp.path()).unwrap();
    let shim_src = std::fs::read_to_string(tmp.path().join("plugin/llmenv-hooks.json")).unwrap();
    assert!(!shim_src.contains("\"event\":\"Notification\""));
}

#[test]
fn materialize_shim_contains_auto_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    OpencodeAdapter
        .materialize(&MergedManifest::default(), tmp.path())
        .unwrap();
    let shim_src = std::fs::read_to_string(tmp.path().join("plugin/llmenv-hooks.json")).unwrap();
    assert!(shim_src.contains("check-stale --engine opencode"));
    assert!(shim_src.contains("config-context --engine opencode"));
    assert!(shim_src.contains("config-guard --engine opencode"));
}

#[test]
fn materialize_hook_with_supported_event_rendered_in_shim() {
    let tmp = tempfile::tempdir().unwrap();
    let mut manifest = MergedManifest::default();
    manifest.capabilities.hooks.push(crate::config::Hook {
        event: "PreToolUse".into(),
        matcher: None,
        handler: crate::config::HookHandler {
            kind: crate::config::HookHandlerKind::Command,
            command: Some("echo hi".into()),
            tool: None,
        },
        bundle_origin: None,
    });
    OpencodeAdapter.materialize(&manifest, tmp.path()).unwrap();
    let shim_src = std::fs::read_to_string(tmp.path().join("plugin/llmenv-hooks.json")).unwrap();
    assert!(shim_src.contains("echo hi"));
    assert!(shim_src.contains("check-stale --engine opencode"));
}

#[test]
fn materialize_no_hooks_still_emits_shim_for_auto_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    OpencodeAdapter
        .materialize(&MergedManifest::default(), tmp.path())
        .unwrap();
    assert!(tmp.path().join("plugin/llmenv.js").exists());
}

#[test]
fn materialize_plugin_hooks_happy_and_warnings() {
    let out = tempfile::tempdir().unwrap();
    let plugin_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(plugin_dir.path().join("hooks")).unwrap();
    std::fs::write(
        plugin_dir.path().join("hooks/hooks.json"),
        serde_json::json!({
            "hooks": {
                "PreToolUse": [{"hooks": [{"type": "command", "command": "echo ok"}]}],
                "Notification": [{"hooks": [{"type": "command", "command": "echo n"}]}],
            }
        })
        .to_string(),
    )
    .unwrap();
    let (plugins, marketplaces) = plugin_manifest(plugin_dir.path());
    let manifest = MergedManifest {
        plugins,
        marketplaces,
        ..Default::default()
    };
    OpencodeAdapter.materialize(&manifest, out.path()).unwrap();
    let shim_src = std::fs::read_to_string(out.path().join("plugin/llmenv-hooks.json")).unwrap();
    assert!(shim_src.contains("echo ok"));
    assert!(!shim_src.contains("echo n"), "Notification is unsupported");
}

#[test]
fn materialize_manifest_hook_mcp_tool_kind_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let mut manifest = MergedManifest::default();
    manifest.capabilities.hooks.push(crate::config::Hook {
        event: "PreToolUse".into(),
        matcher: None,
        handler: crate::config::HookHandler {
            kind: crate::config::HookHandlerKind::McpTool,
            command: None,
            tool: Some("some_tool".into()),
        },
        bundle_origin: None,
    });
    OpencodeAdapter.materialize(&manifest, tmp.path()).unwrap();
    let shim_src = std::fs::read_to_string(tmp.path().join("plugin/llmenv-hooks.json")).unwrap();
    assert!(!shim_src.contains("some_tool"));
}

#[test]
fn todowrite_redirect_follows_the_task_tracker_gate() {
    let tracker = |enabled: bool, block: bool| {
        let mut m = crate::merge::MergedManifest::default();
        m.capabilities.features = Some(crate::config::Features {
            task_tracker: Some(crate::config::TaskTracker {
                enabled,
                block_engine_task_tools: block,
                ..Default::default()
            }),
            ..Default::default()
        });
        m
    };
    let registered = |m: &crate::merge::MergedManifest| {
        let tmp = tempfile::tempdir().unwrap();
        OpencodeAdapter.materialize(m, tmp.path()).unwrap();
        std::fs::read_to_string(tmp.path().join("plugin/llmenv-hooks.json"))
            .unwrap()
            .contains("hook-run pre_tool_use")
    };
    assert!(registered(&tracker(true, true)), "tracker on, redirect on");
    assert!(
        !registered(&tracker(true, false)),
        "block_engine_task_tools off must leave the native todo list alone"
    );
    assert!(!registered(&tracker(false, true)), "tracker off");
}
