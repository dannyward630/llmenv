//! A FIFO at each path that a render, a task list, or a hook reads fails at once, or is skipped
//! with a notice where the code already skips a bad file (#2945).

use std::path::{Path, PathBuf};

use crate::adapter::AgentAdapter as _;
use crate::merge::MergedManifest;

fn fifo(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let made = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .unwrap();
    assert!(made.success());
    path.to_path_buf()
}

fn manifest_with_fifo(src: &Path, rel: &str) -> MergedManifest {
    let mut manifest = MergedManifest::default();
    manifest
        .files
        .insert(PathBuf::from(rel), fifo(&src.join("bundle").join(rel)));
    manifest
}

fn assert_not_regular(err: &anyhow::Error) {
    assert!(format!("{err:#}").contains("not a regular file"), "{err:#}");
}

#[test]
fn the_cache_key_refuses_a_fifo_bundle_file() {
    let src = tempfile::tempdir().unwrap();
    let manifest = manifest_with_fifo(src.path(), "scripts/run.sh");
    assert_not_regular(&crate::materialize::cache::hash_manifest(&manifest).unwrap_err());
}

#[test]
fn the_claude_code_render_refuses_a_fifo_bundle_file() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let manifest = manifest_with_fifo(src.path(), "scripts/run.sh");
    let err = crate::adapter::claude_code::ClaudeCodeAdapter
        .materialize(&manifest, out.path())
        .unwrap_err();
    assert_not_regular(&err);
}

#[test]
fn the_opencode_render_refuses_a_fifo_bundle_command() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let manifest = manifest_with_fifo(src.path(), "commands/run.md");
    let err = crate::adapter::opencode::OpencodeAdapter
        .materialize(&manifest, out.path())
        .unwrap_err();
    assert_not_regular(&err);
}

#[test]
fn the_raw_copy_refuses_a_fifo_bundle_file() {
    let src = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let manifest = manifest_with_fifo(src.path(), "scripts/run.sh");
    // The cache key reads the file first, so either step names the FIFO.
    assert_not_regular(&crate::materialize::materialize(&manifest, cache.path()).unwrap_err());
}

#[test]
fn a_fifo_task_file_is_skipped_with_a_notice() {
    let state = tempfile::tempdir().unwrap();
    fifo(&crate::task::tasks_dir(state.path()).join("stuck.json"));
    assert!(crate::task::list_tasks(state.path()).is_empty());
    let notices = crate::agent_notice::current().unwrap_or_default();
    assert!(notices.contains("stuck.json"), "{notices}");
}

#[test]
fn a_fifo_merge_cache_is_a_miss_with_a_notice() {
    let cache = tempfile::tempdir().unwrap();
    fifo(&cache.path().join("merge-cache.json"));
    assert!(crate::materialize::merge_cache::read_if_matching(cache.path(), "k").is_none());
    let notices = crate::agent_notice::current().unwrap_or_default();
    assert!(notices.contains("merge-cache.json"), "{notices}");
}

#[test]
fn a_fifo_session_log_has_no_count() {
    let dir = tempfile::tempdir().unwrap();
    let log = fifo(&dir.path().join("session.jsonl"));
    assert_eq!(crate::session_log::count::count_records(&log), None);
}

#[test]
fn a_session_log_count_streams_its_records() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("session.jsonl");
    std::fs::write(&log, "{}\n\n{}\n  \n{}\n").unwrap();
    assert_eq!(crate::session_log::count::count_records(&log), Some(3));
}

#[test]
fn a_fifo_marketplace_manifest_is_refused_with_its_path() {
    let market = tempfile::tempdir().unwrap();
    let manifest = fifo(
        &market
            .path()
            .join(".claude-plugin")
            .join("marketplace.json"),
    );
    let err = crate::plugins::cache::read_marketplace_plugins(market.path()).unwrap_err();
    assert_not_regular(&err);
    assert!(
        format!("{err:#}").contains(&manifest.display().to_string()),
        "{err:#}"
    );
}
