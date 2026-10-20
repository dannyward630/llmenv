#![expect(clippy::expect_used, reason = "test scaffolding")]
//! `llmenv setup --scan`, `--tools`, and `--bundles` print onboarding facts as JSON and write
//! nothing (#2785, #2791, #2787).

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

#[test]
fn the_bundle_list_prints_each_shipped_bundle_and_writes_nothing() {
    let home = tempfile::TempDir::new().expect("home");
    let output = isolated_llmenv_cmd(home.path())
        .args(["setup", "--bundles"])
        .output()
        .expect("run llmenv");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let list: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON stdout");
    let names: Vec<&str> = list
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|b| b["name"].as_str())
        .collect();
    assert!(
        names.contains(&"rust-dev") && names.contains(&"python-dev"),
        "{names:?}"
    );
    assert_eq!(
        fs::read_dir(home.path()).expect("list").count(),
        0,
        "it wrote a file"
    );
}

/// The setup skill tells the agent to run `llmenv setup --<flag>`. Each flag it names must exist,
/// or the agent runs a command that fails.
#[test]
fn every_setup_flag_that_the_skill_names_exists() {
    let skill = include_str!("../skills/setup-llmenv/SKILL.md");
    let flags: std::collections::BTreeSet<&str> = skill
        .match_indices("llmenv setup --")
        .filter_map(|(at, prefix)| {
            let rest = skill.get(at + prefix.len()..)?;
            let end = rest
                .find(|c: char| !(c.is_ascii_lowercase() || c == '-'))
                .unwrap_or(rest.len());
            rest.get(..end)
        })
        .collect();
    assert!(
        flags.contains("scan") && flags.contains("tools"),
        "{flags:?}"
    );
    let home = tempfile::TempDir::new().expect("home");
    let output = isolated_llmenv_cmd(home.path())
        .args(["setup", "--help"])
        .output()
        .expect("run llmenv");
    let help = String::from_utf8_lossy(&output.stdout);
    for flag in flags {
        assert!(
            help.contains(&format!("--{flag}")),
            "--{flag} is not a setup flag"
        );
    }
}
