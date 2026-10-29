//! Property tests for reading a plugin manifest. The manifest comes from a plugin folder, so any
//! bytes may appear in it, and a malformed file must give an error, never a panic.

use super::*;
use proptest::prelude::*;

#[expect(
    clippy::expect_used,
    reason = "a test fails loudly when its fixture cannot be set up"
)]
fn write_bytes(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("manifest.json");
    std::fs::write(&path, bytes).expect("write manifest fixture");
    (dir, path)
}

proptest! {
    /// Arbitrary bytes in a hooks manifest return Ok or Err, and never panic.
    #[test]
    fn parse_plugin_hooks_never_panics_on_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let (dir, path) = write_bytes(&bytes);
        let _ = parse_plugin_hooks(&path, dir.path(), "plugin");
    }

    /// Arbitrary bytes in an MCP config return Ok or Err, and never panic.
    #[test]
    fn parse_plugin_mcp_entries_never_panics_on_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let (_dir, path) = write_bytes(&bytes);
        let _ = parse_plugin_mcp_entries(&path, "plugin");
    }
}

/// One generated hook entry: its `type` field and its `command` field.
fn hook_entry() -> impl Strategy<Value = (Option<&'static str>, Option<String>)> {
    let kind = prop_oneof![
        Just(None),
        Just(Some("command")),
        Just(Some("http")),
        Just(Some("prompt")),
    ];
    let command = proptest::option::of(prop_oneof![
        "[a-z ]{0,12}",
        "[a-z ]{0,6}".prop_map(|s| format!("${{CLAUDE_PLUGIN_ROOT}}/{s}")),
        "[a-z]{0,4}".prop_map(|s| format!("${{CLAUDE_PLUGIN_ROOT}} ${{CLAUDE_PLUGIN_ROOT}}{s}")),
    ]);
    (kind, command)
}

proptest! {
    /// Only an entry of type `command` (or no type) with a `command` field gives a hook, and each
    /// hook has every `${CLAUDE_PLUGIN_ROOT}` replaced by the payload path.
    #[test]
    fn parse_plugin_hooks_keeps_only_command_entries_and_resolves_the_root(
        entries in proptest::collection::vec(hook_entry(), 0..12),
    ) {
        let json_entries: Vec<serde_json::Value> = entries
            .iter()
            .map(|(kind, command)| {
                let mut entry = serde_json::Map::new();
                if let Some(kind) = kind {
                    entry.insert("type".into(), (*kind).into());
                }
                if let Some(command) = command {
                    entry.insert("command".into(), command.clone().into());
                }
                serde_json::Value::Object(entry)
            })
            .collect();
        let manifest = serde_json::json!({ "hooks": { "PreToolUse": [{ "hooks": json_entries }] } });
        let (dir, path) = write_bytes(manifest.to_string().as_bytes());
        let payload = dir.path().join("payload");

        let hooks = parse_plugin_hooks(&path, &payload, "plugin")
            .map_err(|e| TestCaseError::fail(format!("{e:#}")))?
            .ok_or_else(|| TestCaseError::fail("the manifest exists, so the result is Some"))?;

        let expected = entries
            .iter()
            .filter(|(kind, command)| matches!(kind, None | Some("command")) && command.is_some())
            .count();
        prop_assert_eq!(hooks.len(), expected);
        for hook in &hooks {
            let command = hook.handler.command.as_deref().unwrap_or_default();
            prop_assert!(!command.contains("${CLAUDE_PLUGIN_ROOT}"), "{command}");
        }
    }
}
