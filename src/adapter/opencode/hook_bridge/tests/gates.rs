//! Gates and hook-table shape, run through the real plugin under `node`.

use super::*;

/// The block a single `failClosed` PreToolUse gate with `command` returns, or a panic when it lets
/// the call through.
fn gate_block(command: &str, matcher: serde_json::Value) -> String {
    let dir = tempfile::tempdir().unwrap();
    let hooks = serde_json::json!([
        { "event": "PreToolUse", "matcher": matcher, "command": command, "timeout": 5000, "failClosed": true },
    ]);
    let plugin = write_plugin(dir.path(), &table(hooks, serde_json::json!({})));
    let driver = r#"
const m = await import(process.argv[1]);
console.log(JSON.stringify(await m.runHooks("PreToolUse", "s", {}, "Bash")));
"#;
    let out = run_node(driver, &[&plugin]);
    out["block"]
        .as_str()
        .unwrap_or_else(|| panic!("the gate let the call through: {out}"))
        .to_string()
}

/// A gate whose program is missing exits 127. That exit must block the call.
#[test]
fn a_gate_with_a_missing_program_blocks_the_call() {
    let block = gate_block("llmenv-no-such-gate-program", serde_json::Value::Null);
    assert!(block.contains("exited with 127"), "{block}");
}

/// A gate that crashes with exit 1 blocks the call, and its stderr names the cause.
#[test]
fn a_crashing_gate_blocks_the_call_and_names_its_stderr() {
    let block = gate_block("echo broke >&2; exit 1", serde_json::Value::Null);
    assert!(block.contains("exited with 1"), "{block}");
    assert!(block.contains("broke"), "{block}");
}

/// A gate killed by a signal has no exit code, and it must still block the call.
#[test]
fn a_gate_killed_by_a_signal_blocks_the_call() {
    let block = gate_block("kill -9 $$", serde_json::Value::Null);
    assert!(block.contains("killed by a signal"), "{block}");
}

/// A gate with a matcher that is not a valid pattern cannot tell which calls it guards, so it
/// blocks them all.
#[test]
fn a_gate_with_a_broken_matcher_blocks_the_call() {
    let block = gate_block("exit 0", serde_json::json!("("));
    assert!(block.contains("not a valid pattern"), "{block}");
}

/// A SessionStart hook that exits 2 is reported on the next request, so the block is not lost.
#[test]
fn a_session_start_block_reaches_the_next_request() {
    let dir = tempfile::tempdir().unwrap();
    let hooks = serde_json::json!([
        { "event": "SessionStart", "matcher": null, "command": "echo blocked by start >&2; exit 2", "timeout": 5000 },
    ]);
    let plugin = write_plugin(dir.path(), &table(hooks, serde_json::json!({})));
    let driver = r#"
const m = await import(process.argv[1]);
const events = [{ type: "session.created", data: { sessionID: "ses_1" } }];
const ctx = {
  location: { directory: process.argv[2] },
  tool: { hook: async () => {} },
  session: { hook: async () => {} },
  event: { subscribe: () => (async function* () { for (const ev of events) yield ev; })() },
};
await m.default.setup(ctx);
// Poll for the block, up to 5 s. The SessionStart hook runs in the background, and systemParts
// hands each text out once, so the texts found so far are kept.
let parts = [];
for (let i = 0; i < 200 && !parts.some((p) => p.text.includes("blocked by start")); i++) {
  await new Promise((resolve) => setTimeout(resolve, 25));
  parts = parts.concat(m.systemParts("ses_1"));
}
console.log(JSON.stringify({ parts }));
"#;
    let out = run_node(driver, &[&plugin, dir.path()]);
    let texts: Vec<&str> = out["parts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["text"].as_str())
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("blocked by start")),
        "{out}"
    );
}

/// A table that parses but lacks `tools` is named on every request, not thrown on a tool call.
#[test]
fn a_table_of_the_wrong_shape_is_named_on_every_request() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin(dir.path(), &serde_json::json!({ "hooks": [] }));
    let driver = r#"
const m = await import(process.argv[1]);
console.log(JSON.stringify({ parts: m.systemParts("s1") }));
"#;
    let out = run_node(driver, &[&plugin]);
    assert!(
        out["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("hook table shape"),
        "{out}"
    );
}

/// With a table that cannot load, no gate runs, so a tool call is refused with the reason.
/// Otherwise a malformed table would turn every gate off without a block.
#[test]
fn a_broken_table_refuses_every_tool_call() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin(dir.path(), &serde_json::json!({ "hooks": [] }));
    let driver = r#"
const m = await import(process.argv[1]);
const hooks = {};
const ctx = {
  location: { directory: process.argv[2] },
  tool: { hook: async (name, cb) => { hooks[name] = cb; } },
  session: { hook: async () => {} },
  event: { subscribe: () => (async function* () {})() },
};
await m.default.setup(ctx);
let message = null;
try {
  await hooks["execute.before"]({ sessionID: "s1", tool: "bash", input: {} });
} catch (e) {
  message = e.message;
}
console.log(JSON.stringify({ message }));
"#;
    let out = run_node(driver, &[&plugin, dir.path()]);
    let message = out["message"]
        .as_str()
        .unwrap_or_else(|| panic!("the call ran: {out}"));
    assert!(message.contains("hook table shape"), "{out}");
}
