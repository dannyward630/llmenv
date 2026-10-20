use super::*;

/// Runs git in `dir` and asserts success. Identity and signing are set per command, so the
/// test does not depend on the developer's global git config.
fn git_run(dir: &Path, args: &[&str]) {
    let status = secure_git()
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

/// Pins the branch to `main`, so the upstream config below names the real branch on every
/// machine, whatever `init.defaultBranch` is set to.
fn git_init(dir: &Path) {
    git_run(dir, &["init", "-q", "-b", "main"]);
}

#[test]
fn working_tree_dirty_is_false_for_a_clean_repo() {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path());
    assert_eq!(working_tree_dirty(dir.path()), Ok(false));
}

#[test]
fn working_tree_dirty_is_true_with_an_untracked_file() {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path());
    std::fs::write(dir.path().join("new.txt"), b"x").unwrap();
    assert_eq!(working_tree_dirty(dir.path()), Ok(true));
}

#[test]
fn working_tree_dirty_errors_outside_a_repo_instead_of_reporting_clean() {
    let dir = tempfile::tempdir().unwrap();
    let err = working_tree_dirty(dir.path()).unwrap_err();
    assert!(err.contains("git status failed"), "unexpected error: {err}");
}

#[test]
fn has_unpushed_commits_is_false_when_the_branch_has_no_upstream() {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path());
    // An unborn HEAD reports "no such branch", not a missing upstream, so commit first.
    git_run(dir.path(), &["commit", "-q", "--allow-empty", "-m", "init"]);
    assert_eq!(has_unpushed_commits(dir.path()), Ok(false));
}

#[test]
fn has_unpushed_commits_errors_when_the_upstream_ref_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path());
    git_run(dir.path(), &["commit", "-q", "--allow-empty", "-m", "init"]);
    // The branch names an upstream, but no remote-tracking ref backs it. That is a real
    // failure, not "no upstream", so it must not read as nothing to push.
    git_run(dir.path(), &["config", "branch.main.remote", "origin"]);
    git_run(
        dir.path(),
        &["config", "branch.main.merge", "refs/heads/main"],
    );
    let err = has_unpushed_commits(dir.path()).unwrap_err();
    assert!(
        err.contains("git rev-list failed"),
        "unexpected error: {err}"
    );
}

#[test]
fn has_unpushed_commits_errors_outside_a_repo_instead_of_reporting_none() {
    let dir = tempfile::tempdir().unwrap();
    let err = has_unpushed_commits(dir.path()).unwrap_err();
    assert!(
        err.contains("git rev-list failed"),
        "unexpected error: {err}"
    );
}
