//! Tests for the pin check. Each test builds a real clone under a temporary cache root,
//! because the check reads the clone's HEAD and refs through git.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test fails loudly when its setup fails"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use super::pin_problem;

/// Runs `git` in `dir` with a fixed identity, and fails the test when git fails.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

/// A cache root holding one clone named `name`, with a commit tagged `v1`.
/// Returns the cache root and the clone path.
fn clone_with_tag(name: &str, annotated: bool) -> (tempfile::TempDir, PathBuf) {
    let cache = tempfile::tempdir().unwrap();
    let clone = cache.path().join("marketplaces").join(name);
    std::fs::create_dir_all(&clone).unwrap();
    git(&clone, &["init", "-q", "-b", "main"]);
    std::fs::write(clone.join("README.md"), b"one").unwrap();
    git(&clone, &["add", "README.md"]);
    git(&clone, &["commit", "-q", "-m", "one"]);
    if annotated {
        git(&clone, &["tag", "-a", "v1", "-m", "release one"]);
    } else {
        git(&clone, &["tag", "v1"]);
    }
    (cache, clone)
}

/// Moves the clone's HEAD past the `v1` tag with one more commit.
fn advance_head(clone: &Path) {
    std::fs::write(clone.join("README.md"), b"two").unwrap();
    git(clone, &["commit", "-q", "-am", "two"]);
}

#[test]
fn a_source_without_a_ref_has_no_pin_to_check() {
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(
        pin_problem("mkt", "https://example.com/mkt.git", cache.path()),
        None
    );
}

#[test]
fn a_pin_with_no_local_clone_is_skipped() {
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(
        pin_problem("mkt", "https://example.com/mkt.git#v1", cache.path()),
        None
    );
}

#[test]
fn a_pin_that_matches_head_reports_nothing() {
    let (cache, _clone) = clone_with_tag("mkt", false);
    let source = "https://example.com/mkt.git#v1";
    assert_eq!(pin_problem("mkt", source, cache.path()), None);
}

#[test]
fn an_annotated_tag_pin_is_peeled_to_its_commit() {
    // #695: an annotated tag names a tag object, not the commit. The check peels
    // it, so a clone on that commit must not warn.
    let (cache, _clone) = clone_with_tag("mkt", true);
    let source = "https://example.com/mkt.git#v1";
    assert_eq!(pin_problem("mkt", source, cache.path()), None);
}

#[test]
fn a_head_past_the_pin_names_both_commits() {
    let (cache, clone) = clone_with_tag("mkt", false);
    advance_head(&clone);
    let problem = pin_problem("mkt", "https://example.com/mkt.git#v1", cache.path())
        .expect("a moved HEAD must be reported");
    assert!(
        problem.starts_with("marketplace 'mkt' pinned to 'v1': HEAD ("),
        "{problem}"
    );
    assert!(
        problem.contains("does not match the pinned ref's commit"),
        "{problem}"
    );
    assert!(
        problem.ends_with("run `llmenv plugin-sync` to repair"),
        "{problem}"
    );
}

#[test]
fn a_pin_that_names_no_ref_in_the_clone_is_reported() {
    let (cache, _clone) = clone_with_tag("mkt", false);
    let problem = pin_problem(
        "mkt",
        "https://example.com/mkt.git#no-such-tag",
        cache.path(),
    )
    .expect("an unresolvable pin must be reported");
    assert!(
        problem.contains("pinned to 'no-such-tag' cannot be resolved"),
        "{problem}"
    );
}

#[test]
fn a_clone_without_a_readable_head_is_reported() {
    // A `.git` folder with no repository in it: `git rev-parse HEAD` fails.
    let cache = tempfile::tempdir().unwrap();
    let clone = cache.path().join("marketplaces").join("mkt");
    std::fs::create_dir_all(clone.join(".git")).unwrap();
    let problem = pin_problem("mkt", "https://example.com/mkt.git#v1", cache.path())
        .expect("an unreadable HEAD must be reported");
    assert!(
        problem.contains("cannot read HEAD in its local clone"),
        "{problem}"
    );
}
