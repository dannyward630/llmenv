#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use super::{candidates, find_on_path, report};

/// Package managers whose commands the plan can show.
const KNOWN_MANAGERS: &[&str] = &["apt", "brew", "cargo", "npm", "script", "uv"];

#[cfg(unix)]
fn executable(dir: &Path, name: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_data_file_parses_and_every_tool_has_a_binary_and_an_install_command() {
    let tools = candidates().unwrap();
    assert!(!tools.is_empty());
    let mut names = BTreeSet::new();
    for tool in &tools {
        assert!(
            names.insert(tool.name.clone()),
            "duplicate tool {}",
            tool.name
        );
        assert!(!tool.binaries.is_empty(), "{} has no binary", tool.name);
        assert!(
            !tool.install.is_empty(),
            "{} has no install command",
            tool.name
        );
        assert!(
            !tool.purpose.is_empty() && !tool.evidence.is_empty(),
            "{}",
            tool.name
        );
        for manager in tool.install.keys() {
            assert!(
                KNOWN_MANAGERS.contains(&manager.as_str()),
                "{}: {manager}",
                tool.name
            );
        }
    }
}

#[test]
fn icm_and_codebase_memory_come_first_as_the_strong_default() {
    let tools = candidates().unwrap();
    let first: Vec<_> = tools.iter().take(2).map(|t| t.name.as_str()).collect();
    assert_eq!(first, ["icm", "codebase-memory-mcp"]);
    assert!(tools.iter().take(2).all(|t| t.strong_default));
    assert!(tools.iter().skip(2).all(|t| !t.strong_default));
    assert_eq!(tools[0].feature.as_deref(), Some("memory"));
    assert_eq!(tools[1].feature.as_deref(), Some("codebase_memory"));
}

#[test]
fn no_install_command_pins_a_version() {
    for tool in candidates().unwrap() {
        for command in tool.install.values() {
            let pinned = command.contains("==")
                || command.contains("--version")
                || command.split_whitespace().any(|word| {
                    word.rsplit_once('@')
                        .is_some_and(|(_, v)| v.starts_with(|c: char| c.is_ascii_digit()))
                });
            assert!(!pinned, "{}: {command}", tool.name);
        }
    }
}

#[cfg(unix)]
#[test]
fn the_first_executable_on_path_is_found_and_a_plain_file_is_not() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    executable(second.path(), "rg");
    std::fs::write(first.path().join("rg"), "not executable").unwrap();
    let path = std::env::join_paths([first.path(), second.path()]).unwrap();

    assert_eq!(
        find_on_path("rg", Some(&path)),
        Some(second.path().join("rg"))
    );
    assert_eq!(find_on_path("absent", Some(&path)), None);
    assert_eq!(find_on_path("rg", None), None);
    assert_eq!(
        find_on_path("../rg", Some(&path)),
        None,
        "a path is not a name"
    );
}

#[cfg(unix)]
#[test]
fn the_report_marks_each_tool_and_names_the_binary_it_found() {
    let dir = tempfile::tempdir().unwrap();
    executable(dir.path(), "fdfind");
    let path: OsString = dir.path().as_os_str().to_owned();

    let report = report(Some(&path)).unwrap();

    let fd = report.tools.iter().find(|t| t.tool.name == "fd").unwrap();
    assert_eq!(
        fd.found.as_deref(),
        Some(dir.path().join("fdfind").as_path())
    );
    let rg = report
        .tools
        .iter()
        .find(|t| t.tool.name == "ripgrep")
        .unwrap();
    assert!(rg.found.is_none());
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["tools"][0]["name"], "icm");
    assert!(json["tools"][0].get("found").is_some());
}
