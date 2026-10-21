//! Bus events and subagent calls, run through the real plugin under `node`.

use super::*;

/// Drives the bus-event and subagent bindings. The events arrive from the event stream, and a
/// `subagent` call runs in the foreground (`completed`) and in the background (`running`).
const BUS_DRIVER: &str = r#"
const plugin = (await import(process.argv[1])).default;
const hooks = {};
const events = [
  { type: "session.created", data: { sessionID: "ses_1" } },
  { type: "permission.asked", data: { sessionID: "ses_1", id: "p1", action: "shell", resources: ["rm"] } },
  { type: "session.compaction.started", data: { sessionID: "ses_1", reason: "auto", recent: "" } },
  {
    type: "session.model.selected",
    data: {
      sessionID: "ses_1",
      model: { providerID: "anthropic", id: "claude", variant: "high" },
      previous: { providerID: "openai", id: "gpt" },
    },
  },
  { type: "session.step.ended", data: { sessionID: "ses_1" } },
];
const ctx = {
  location: { directory: process.argv[2] },
  tool: { hook: async (name, cb) => { hooks["tool." + name] = cb; } },
  session: { hook: async (name, cb) => { hooks["session." + name] = cb; } },
  event: { subscribe: () => (async function* () { for (const ev of events) yield ev; })() },
};
await plugin.setup(ctx);
const call = { sessionID: "ses_1", agent: "build", messageID: "m", tool: "subagent" };
const input = { agent: "explore", description: "find it", prompt: "look" };
await hooks["tool.execute.before"]({ ...call, id: "s1", input });
await hooks["tool.execute.after"]({
  ...call, id: "s1", input, status: "completed", result: { sessionID: "c1", status: "completed", output: "found it" },
});
await hooks["tool.execute.after"]({
  ...call, id: "s2", input, status: "completed", result: { sessionID: "c2", status: "running", output: "" },
});
// Poll until the six payloads are logged and the Notification text is delivered. The event
// stream runs on its own schedule, so a fixed sleep is too short under load.
const { readFileSync } = await import("node:fs");
const logged = () => {
  try {
    return readFileSync(process.argv[3], "utf8").trim().split("\n").length;
  } catch {
    return 0;
  }
};
const system = [];
for (let i = 0; i < 500 && !(logged() >= 6 && system.some((p) => p.text === "notify note")); i++) {
  await new Promise((r) => setTimeout(r, 10));
  await hooks["session.context"]({ sessionID: "ses_1", system });
}
console.log(JSON.stringify(system.map((p) => p.text)));
"#;

#[test]
fn the_plugin_bridges_bus_events_and_subagent_stops() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("payloads.jsonl");
    let log_s = log.display();
    // One printf writes a whole line, so hooks that run at once cannot interleave their payloads.
    let record = format!("p=$(cat); printf '%s\\n' \"$p\" >> {log_s}");
    let note = r#"{"hookSpecificOutput":{"additionalContext":"notify note"}}"#;
    let hooks = vec![
        hook("SubagentStart", None, &record),
        hook("SubagentStop", None, &record),
        hook("Notification", None, &format!("{record}; echo '{note}'")),
        hook("PreCompact", None, &record),
        hook("PostModelSwitch", None, &record),
        hook("PostToolBatch", None, &record),
    ];
    let mut table = hook_table(&hooks, &[], &["codebase-memory-mcp"]);
    table["hooks"]
        .as_array_mut()
        .unwrap()
        .retain(|h| !h["command"].as_str().unwrap().starts_with("llmenv "));
    let plugin = write_plugin(dir.path(), &table);
    let system = run_node(BUS_DRIVER, &[&plugin, dir.path(), log.as_path()]);

    let payloads: Vec<serde_json::Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let named = |name: &str| -> Vec<&serde_json::Value> {
        payloads
            .iter()
            .filter(|p| p["hook_event_name"] == name)
            .collect()
    };

    let start = named("SubagentStart");
    assert_eq!(start.len(), 1);
    assert_eq!(start[0]["agent_type"], "explore");
    assert_eq!(start[0]["prompt"], "look");
    // A background run returns "running" at launch, so only the foreground run stops here.
    let stop = named("SubagentStop");
    assert_eq!(
        stop.len(),
        1,
        "a background subagent must not report a stop"
    );
    assert_eq!(stop[0]["last_assistant_message"], "found it");
    assert_eq!(
        named("Notification")[0]["message"],
        "Permission needed for shell: rm"
    );
    assert_eq!(named("PreCompact")[0]["trigger"], "auto");
    let switch = named("PostModelSwitch");
    assert_eq!(switch[0]["from_model"], "openai/gpt");
    assert_eq!(switch[0]["to_model"], "anthropic/claude#high");
    assert_eq!(named("PostToolBatch").len(), 1);
    // A context from a bus-event hook reaches the next request, as a Stop reminder does.
    assert!(
        system
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "notify note")
    );
}

/// The text of a Stop hook reaches the next request, one time.
#[test]
fn a_stop_reminder_reaches_the_next_request() {
    let dir = tempfile::tempdir().unwrap();
    let hooks = serde_json::json!([
        { "event": "Stop", "matcher": null, "command": "echo stop note", "timeout": 5000 },
    ]);
    let plugin = write_plugin(dir.path(), &table(hooks, serde_json::json!({})));
    let driver = r#"
const plugin = (await import(process.argv[1])).default;
const m = await import(process.argv[1]);
const hooks = {};
const ctx = {
  location: { directory: process.argv[2] },
  tool: { hook: async (name, cb) => { hooks["tool." + name] = cb; } },
  session: { hook: async (name, cb) => { hooks["session." + name] = cb; } },
  event: { subscribe: () => (async function* () {
    yield { type: "session.created", data: { sessionID: "ses_1" } };
    yield { type: "session.execution.succeeded", data: { sessionID: "ses_1" } };
  })() },
};
await plugin.setup(ctx);
// Poll for the Stop text, because the event stream runs on its own schedule. A fixed sleep is
// too short when the suite runs many tests at once.
const first = { sessionID: "ses_1", system: [] };
for (let i = 0; i < 500 && !first.system.some((p) => p.text === "stop note"); i++) {
  await new Promise((r) => setTimeout(r, 10));
  first.system.length = 0;
  await hooks["session.context"](first);
}
const second = { sessionID: "ses_1", system: [] };
await hooks["session.context"](second);
console.log(JSON.stringify({ first: first.system.map((p) => p.text), second: second.system.map((p) => p.text) }));
"#;
    let out = run_node(driver, &[&plugin, dir.path()]);
    assert!(
        out["first"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("stop note")),
        "{out}"
    );
    assert!(
        !out["second"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("stop note")),
        "{out}"
    );
}
