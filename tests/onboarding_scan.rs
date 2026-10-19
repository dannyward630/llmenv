#![expect(clippy::expect_used, reason = "test scaffolding")]
//! `llmenv setup --scan` prints the onboarding scan as JSON and writes nothing (#2785).

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
