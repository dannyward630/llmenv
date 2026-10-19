#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use super::{Bounds, ScanReport, scan};
use crate::onboarding::scan::MAX_NAMES;

pub(super) fn repo(root: &Path, rel: &str) -> PathBuf {
    let dir = root.join(rel);
    fs::create_dir_all(dir.join(".git")).unwrap();
    dir
}

/// Scan with a home folder that is not above any test root.
pub(super) fn scan_with(roots: &[PathBuf], bounds: Bounds) -> anyhow::Result<ScanReport> {
    let home = tempfile::tempdir().unwrap();
    scan(roots, home.path(), bounds)
}

pub(super) fn run(roots: &[&Path]) -> ScanReport {
    let roots: Vec<PathBuf> = roots.iter().map(|r| r.to_path_buf()).collect();
    scan_with(&roots, Bounds::DEFAULT).unwrap()
}

pub(super) fn paths(report: &ScanReport) -> Vec<PathBuf> {
    report.repositories.iter().map(|r| r.path.clone()).collect()
}

#[test]
fn a_repository_reports_its_names_manifest_keys_and_workflows() {
    let root = tempfile::tempdir().unwrap();
    let dir = repo(root.path(), "app");
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"app\"\nedition = \"2024\"\nrust-version = \"1.90\"\n\n[dependencies]\nserde = \"1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"scripts": {"test": "rm -rf / && echo", "lint": "x"}, "packageManager": "pnpm@9"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("pyproject.toml"),
        "[project]\nname = \"p\"\nrequires-python = \">=3.12\"\n\n[tool.ruff]\nline-length = 100\n\n[tool.pytest.ini_options]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join(".github/workflows")).unwrap();
    fs::write(dir.join(".github/workflows/ci.yml"), "on: push").unwrap();
    fs::write(dir.join("main.rs"), "fn main() {}").unwrap();

    let report = run(&[root.path()]);

    assert_eq!(report.repositories.len(), 1);
    let found = &report.repositories[0];
    assert_eq!(found.path, fs::canonicalize(&dir).unwrap());
    assert!(
        found.names.contains(&"Cargo.toml".to_string()),
        "{:?}",
        found.names
    );
    assert!(
        found.names.contains(&".github/".to_string()),
        "{:?}",
        found.names
    );
    assert_eq!(found.workflows, vec!["ci.yml".to_string()]);
    let cargo = found.cargo.as_ref().unwrap();
    assert_eq!(cargo.name.as_deref(), Some("app"));
    assert_eq!(cargo.edition.as_deref(), Some("2024"));
    assert_eq!(cargo.rust_version.as_deref(), Some("1.90"));
    assert!(!cargo.workspace);
    let package = found.package_json.as_ref().unwrap();
    assert_eq!(
        package.scripts,
        vec!["lint".to_string(), "test".to_string()]
    );
    assert_eq!(package.package_manager.as_deref(), Some("pnpm@9"));
    let pyproject = found.pyproject.as_ref().unwrap();
    assert_eq!(
        pyproject.tools,
        vec!["pytest".to_string(), "ruff".to_string()]
    );
    assert_eq!(pyproject.requires_python.as_deref(), Some(">=3.12"));
    let json = serde_json::to_string(&report).unwrap();
    assert!(
        !json.contains("rm -rf"),
        "a script body leaked into the scan: {json}"
    );
    assert!(
        !json.contains("fn main"),
        "source text leaked into the scan: {json}"
    );
}

#[test]
fn the_walk_skips_hidden_and_build_folders_and_does_not_enter_a_repository() {
    let root = tempfile::tempdir().unwrap();
    for skipped in [
        ".cache",
        "node_modules",
        "target",
        ".venv",
        "vendor",
        "dist",
        "build",
    ] {
        repo(root.path(), &format!("{skipped}/inner"));
    }
    let outer = repo(root.path(), "outer");
    repo(&outer, "nested");

    let report = run(&[root.path()]);

    assert_eq!(paths(&report), vec![fs::canonicalize(&outer).unwrap()]);
}

#[test]
fn a_repository_named_like_a_skipped_folder_is_still_reported() {
    let root = tempfile::tempdir().unwrap();
    let build = repo(root.path(), "build");
    let hidden = repo(root.path(), ".dotfiles");

    let report = run(&[root.path()]);

    let mut expected = vec![
        fs::canonicalize(&build).unwrap(),
        fs::canonicalize(&hidden).unwrap(),
    ];
    expected.sort();
    assert_eq!(paths(&report), expected);
}

#[test]
fn an_inherited_value_reads_as_workspace_and_a_wrong_type_is_a_note() {
    let root = tempfile::tempdir().unwrap();
    let dir = repo(root.path(), "app");
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = 7\nedition.workspace = true\nrust-version = { workspace = true }\n",
    )
    .unwrap();

    let report = run(&[root.path()]);

    let found = &report.repositories[0];
    let cargo = found.cargo.as_ref().unwrap();
    assert_eq!(cargo.edition.as_deref(), Some("workspace"));
    assert_eq!(cargo.rust_version.as_deref(), Some("workspace"));
    assert!(cargo.name.is_none());
    assert!(
        found.notes.iter().any(|n| n.contains("package.name")),
        "{:?}",
        found.notes
    );
}

#[test]
fn a_long_script_list_is_cut_with_a_note() {
    let root = tempfile::tempdir().unwrap();
    let dir = repo(root.path(), "app");
    let scripts: serde_json::Map<String, serde_json::Value> = (0..=MAX_NAMES)
        .map(|i| (format!("s{i:04}"), serde_json::Value::from("x")))
        .collect();
    fs::write(
        dir.join("package.json"),
        serde_json::json!({ "scripts": scripts }).to_string(),
    )
    .unwrap();

    let report = run(&[root.path()]);

    let found = &report.repositories[0];
    assert_eq!(
        found.package_json.as_ref().unwrap().scripts.len(),
        MAX_NAMES
    );
    assert!(
        found.notes.iter().any(|n| n.contains("MAX_NAMES")),
        "{:?}",
        found.notes
    );
}

#[test]
fn a_repository_below_the_depth_bound_is_counted_and_not_reported() {
    let root = tempfile::tempdir().unwrap();
    repo(root.path(), "a/b/c/d/e/deep");
    let shallow = repo(root.path(), "a/near");

    let report = run(&[root.path()]);

    assert_eq!(paths(&report), vec![fs::canonicalize(&shallow).unwrap()]);
    assert!(report.skipped.below_depth_bound > 0);
}

#[test]
fn a_root_that_is_a_repository_is_reported() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    let report = run(&[root.path()]);
    assert_eq!(paths(&report), vec![fs::canonicalize(root.path()).unwrap()]);
}

#[test]
fn a_missing_root_is_an_error_that_names_it() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("absent");
    let error = scan_with(&[missing.clone()], Bounds::DEFAULT).unwrap_err();
    assert!(format!("{error:#}").contains("absent"), "{error:#}");
}

#[test]
fn more_repositories_than_the_bound_stop_the_scan_and_name_the_constant() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..3 {
        repo(root.path(), &format!("r{i}"));
    }
    let bounds = Bounds {
        max_repositories: 2,
        ..Bounds::DEFAULT
    };
    let error = scan_with(&[root.path().to_path_buf()], bounds).unwrap_err();
    assert!(
        format!("{error:#}").contains("MAX_REPOSITORIES"),
        "{error:#}"
    );
}

#[test]
fn more_folders_than_the_bound_stop_the_scan_and_name_the_constant() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..5 {
        fs::create_dir(root.path().join(format!("d{i}"))).unwrap();
    }
    let bounds = Bounds {
        max_visited_dirs: 3,
        ..Bounds::DEFAULT
    };
    let error = scan_with(&[root.path().to_path_buf()], bounds).unwrap_err();
    assert!(
        format!("{error:#}").contains("MAX_VISITED_DIRS"),
        "{error:#}"
    );
}

#[test]
fn a_malformed_or_large_manifest_is_a_note_not_an_error() {
    let root = tempfile::tempdir().unwrap();
    let dir = repo(root.path(), "app");
    fs::write(dir.join("Cargo.toml"), "[package\nname = ").unwrap();
    let big = "x".repeat(usize::try_from(super::MAX_MANIFEST_BYTES).unwrap() + 1);
    fs::write(dir.join("package.json"), big).unwrap();

    let report = run(&[root.path()]);

    let found = &report.repositories[0];
    assert!(found.cargo.is_none() && found.package_json.is_none());
    assert!(
        found.notes.iter().any(|n| n.contains("Cargo.toml")),
        "{:?}",
        found.notes
    );
    assert!(
        found.notes.iter().any(|n| n.contains("MAX_MANIFEST_BYTES")),
        "{:?}",
        found.notes
    );
}

#[test]
fn a_long_manifest_value_is_cut() {
    let root = tempfile::tempdir().unwrap();
    let dir = repo(root.path(), "app");
    let long = "n".repeat(super::MAX_VALUE_CHARS * 2);
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{long}\"\n"),
    )
    .unwrap();

    let report = run(&[root.path()]);

    let name = report.repositories[0]
        .cargo
        .as_ref()
        .unwrap()
        .name
        .clone()
        .unwrap();
    assert_eq!(name.chars().count(), super::MAX_VALUE_CHARS);
}

#[test]
fn repositories_are_reported_once_and_in_path_order() {
    let root = tempfile::tempdir().unwrap();
    let b = repo(root.path(), "b");
    let a = repo(root.path(), "a");
    let report = run(&[root.path(), root.path()]);
    let expected = vec![fs::canonicalize(&a).unwrap(), fs::canonicalize(&b).unwrap()];
    assert_eq!(paths(&report), expected);
}

/// Whether a repository at `rel` (relative to the root) is one the walk must report: within the
/// depth bound, no folder above it skipped by name, and no repository above it.
fn reachable(rel: &[String], all: &std::collections::BTreeSet<Vec<String>>) -> bool {
    let depth_ok = rel.len() <= super::MAX_DEPTH;
    let parents = &rel[..rel.len().saturating_sub(1)];
    // The rule is written out again here, so a broken `enters` cannot agree with itself.
    let skipped = ["node_modules", "target", "vendor", "dist", "build"];
    let entered = parents
        .iter()
        .all(|name| !name.starts_with('.') && !skipped.contains(&name.as_str()));
    let nested = (1..rel.len()).any(|n| all.contains(&rel[..n].to_vec()));
    depth_ok && entered && !nested
}

#[test]
fn the_walk_reports_exactly_the_reachable_repositories() {
    use proptest::prelude::*;
    let segment = prop::sample::select(vec!["a", "b", "build", ".h", "node_modules"]);
    let path = proptest::collection::vec(segment, 1..7);
    proptest!(ProptestConfig::with_cases(48), |(created in proptest::collection::vec(path, 0..6))| {
        let root = tempfile::tempdir().unwrap();
        let all: std::collections::BTreeSet<Vec<String>> = created
            .iter()
            .map(|p| p.iter().map(|s| (*s).to_string()).collect())
            .collect();
        for rel in &all {
            fs::create_dir_all(root.path().join(rel.join("/")).join(".git")).unwrap();
        }
        let base = fs::canonicalize(root.path()).unwrap();
        let expected: Vec<PathBuf> = all
            .iter()
            .filter(|rel| reachable(rel, &all))
            .map(|rel| base.join(rel.join("/")))
            .collect();

        let report = run(&[root.path()]);

        prop_assert_eq!(paths(&report), expected);
    });
}
