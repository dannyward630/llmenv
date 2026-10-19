#![expect(clippy::expect_used, reason = "test scaffolding")]
//! `llmenv setup --scan` and `--tools` print onboarding facts as JSON and write nothing (#2785,
//! #2791).

mod support;

use std::fs;

use support::isolated_llmenv_cmd;

#[test]
fn the_scan_prints_json_evidence_and_writes_nothing() {
    let home = tempfile::TempDir::new().expect("home");
    let root = home.path().join("git");
    fs::create_dir_all(root.join("app/.git")).expect("mkdir repo");
    fs::write(root.join("app/Cargo.toml"), "[package]\nname = \"app\"\n").expect("manifest");
    let before = fs::read_dir(home.path()).expect("list").count();

    let output = isolated_llmenv_cmd(home.path())
        .args(["setup", "--scan"])
        .arg(&root)
        .output()
        .expect("run llmenv");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON stdout");
    assert_eq!(report["repositories"][0]["cargo"]["name"], "app");
    assert_eq!(report["limits"]["max_depth"], 4);
    assert_eq!(fs::read_dir(home.path()).expect("list").count(), before);
}

#[test]
fn the_scan_refuses_the_home_folder() {
    let home = tempfile::TempDir::new().expect("home");
    let output = isolated_llmenv_cmd(home.path())
        .args(["setup", "--scan"])
        .arg(home.path())
        .output()
        .expect("run llmenv");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("narrower"));
}

#[test]
fn the_scan_does_not_combine_with_the_wizard_flags() {
    let home = tempfile::TempDir::new().expect("home");
    let output = isolated_llmenv_cmd(home.path())
        .args(["setup", "--rescan", "--scan", "x"])
        .output()
        .expect("run llmenv");
    assert!(!output.status.success());
}

#[test]
fn the_tool_report_lists_the_strong_defaults_first_and_reads_path() {
    let home = tempfile::TempDir::new().expect("home");
    let output = isolated_llmenv_cmd(home.path())
        .args(["setup", "--tools"])
        .env("PATH", "")
        .output()
        .expect("run llmenv");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON stdout");
    assert_eq!(report["tools"][0]["name"], "icm");
    assert_eq!(report["tools"][0]["strong_default"], true);
    assert!(
        report["tools"][0]["found"].is_null(),
        "an empty PATH found a binary"
    );
}
