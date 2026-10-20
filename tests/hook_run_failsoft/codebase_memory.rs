//! The codebase-memory fail-soft and index-guard cases of `hook_run_failsoft`.

use super::*;

pub(super) fn config_with_codebase_memory() -> String {
    format!(
        r#"
scope:
  network: []
  host: []
  user:
    - id: test-user
      match:
        user: {user}
      tags: [test]

tag:
  test: ""

bundle:
  - name: test-bundle
    when: [test]

features:
  codebase_memory:
    - when: [test]

cache:
  sync_interval_minutes: 60

adapter:
  engine: claude-code
"#,
        user = current_user(),
    )
}

// #365: SessionStart with an active codebase_memory scope must not block or
// fail even when the codebase-memory-mcp binary isn't installed (fail-soft
// contract — the spawn attempt itself must never surface as a hook failure).
#[test]
fn session_start_with_codebase_memory_exits_zero_without_binary() {
    let (dir, config_path) = setup_config(&config_with_codebase_memory());
    hook_cmd(dir.path(), &config_path, "session_start")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
}

/// Put a fake `codebase-memory-mcp` with the given shell body first on `PATH`.
#[cfg(unix)]
pub(super) fn with_fake_cbm(cmd: &mut Command, bin_dir: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let script = bin_dir.join("codebase-memory-mcp");
    fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let old_path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{}:{old_path}", bin_dir.display()));
}

// #2358: a codebase-memory server that exits at once is named on stdout, with its fix.
#[cfg(unix)]
#[test]
fn session_start_names_a_dead_codebase_memory_server() {
    let (dir, config_path) = setup_config(&config_with_codebase_memory());
    let bin = TempDir::new().unwrap();
    let mut cmd = hook_cmd(dir.path(), &config_path, "session_start");
    with_fake_cbm(&mut cmd, bin.path(), "exit 1");
    cmd.timeout(Duration::from_secs(20))
        .assert()
        .success()
        .stdout(predicate::str::contains("MCP health check failed"))
        .stdout(predicate::str::contains("codebase-memory-mcp"))
        .stdout(predicate::str::contains("cbm-daemon-internal"));
}

// #2358: a healthy server adds nothing to the session start output.
#[cfg(unix)]
#[test]
fn session_start_is_silent_when_codebase_memory_answers() {
    let (dir, config_path) = setup_config(&config_with_codebase_memory());
    let bin = TempDir::new().unwrap();
    let mut cmd = hook_cmd(dir.path(), &config_path, "session_start");
    let reply = r#"{"jsonrpc":"2.0","id":0,"result":{}}"#;
    with_fake_cbm(
        &mut cmd,
        bin.path(),
        &format!("read line\nprintf '%s\\n' '{reply}'"),
    );
    cmd.timeout(Duration::from_secs(20))
        .assert()
        .success()
        .stdout(predicate::str::contains("MCP health check failed").not());
}

/// A fake server that records each `allow-root <path>` call in `log` and lists no roots. An
/// empty record is unconfined, so the session has nothing to report.
#[cfg(unix)]
fn recording_cbm(log: &std::path::Path, add_status: i32) -> String {
    format!(
        "if [ \"$1\" = allow-root ]; then\n\
         if [ \"$2\" = --list ]; then echo 'allowed roots:'; exit 0; fi\n\
         echo \"$2\" >> '{}'\n\
         echo 'refused by the fake server' >&2\n\
         exit {add_status}\n\
         fi\n\
         read line\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{{}}}}'",
        log.display()
    )
}

/// A fake server that lists one recorded root and answers the MCP handshake.
#[cfg(unix)]
fn listing_cbm(root: &str) -> String {
    format!(
        "if [ \"$1\" = allow-root ]; then echo 'allowed roots:'; echo '{root}'; exit 0; fi\n\
         read line\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":0,\"result\":{{}}}}'"
    )
}

// #2916: SessionStart only reads the server's roots. It never records one.
#[cfg(unix)]
#[test]
fn session_start_never_runs_allow_root() {
    let (dir, config_path) = setup_config(&config_with_codebase_memory());
    let bin = TempDir::new().unwrap();
    let log = bin.path().join("roots.log");
    let mut cmd = hook_cmd(dir.path(), &config_path, "session_start");
    cmd.current_dir(dir.path());
    with_fake_cbm(&mut cmd, bin.path(), &recording_cbm(&log, 0));
    cmd.timeout(Duration::from_secs(20))
        .assert()
        .success()
        .stdout(predicate::str::contains("cannot index").not());
    assert!(
        !log.exists(),
        "SessionStart ran allow-root: {:?}",
        fs::read_to_string(&log)
    );
}

// #2916: a declared root that the server lacks is named on stdout, with the command for the user.
#[cfg(unix)]
#[test]
fn session_start_names_a_declared_root_the_server_lacks() {
    let declared = std::env::temp_dir();
    let config = config_with_codebase_memory().replace(
        "    - when: [test]",
        &format!(
            "    - when: [test]\n      allowed_roots: [\"{}\"]",
            declared.display()
        ),
    );
    let (dir, config_path) = setup_config(&config);
    let bin = TempDir::new().unwrap();
    let mut cmd = hook_cmd(dir.path(), &config_path, "session_start");
    cmd.current_dir(dir.path());
    with_fake_cbm(&mut cmd, bin.path(), &listing_cbm("/nonexistent-root-2916"));
    cmd.timeout(Duration::from_secs(20))
        .assert()
        .success()
        .stdout(predicate::str::contains("cannot index"))
        .stdout(predicate::str::contains("allow-root"))
        .stdout(predicate::str::contains("llmenv never runs"));
}

// #2916: an index_repository call outside the recorded roots is denied, and the reason names the
// command the user runs.
#[cfg(unix)]
#[test]
fn pre_tool_use_denies_an_index_outside_the_recorded_roots() {
    let (dir, config_path) = setup_config(&config_with_codebase_memory());
    let bin = TempDir::new().unwrap();
    let mut cmd = hook_cmd(dir.path(), &config_path, "pre_tool_use");
    cmd.current_dir(dir.path());
    with_fake_cbm(
        &mut cmd,
        bin.path(),
        &listing_cbm(&std::env::temp_dir().display().to_string()),
    );
    let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"mcp__codebase-memory-mcp__index_repository","tool_input":{"repo_path":"/usr"}}"#;
    cmd.timeout(Duration::from_secs(20))
        .write_stdin(payload)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "outside the codebase-memory allowed roots",
        ))
        .stdout(predicate::str::contains("allow-root"));
}
