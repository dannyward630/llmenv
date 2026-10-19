#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Scan tests for the safety rules: symlinks, special files, unreadable roots, and the refused
//! roots.

use std::fs;
use std::path::PathBuf;

use super::tests::{paths, repo, run, scan_with};
use super::{Bounds, scan};

#[cfg(unix)]
#[test]
fn a_symlink_is_not_followed() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    repo(elsewhere.path(), "far");
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join("link")).unwrap();

    let report = run(&[root.path()]);

    assert!(report.repositories.is_empty(), "{:?}", paths(&report));
}

#[cfg(unix)]
#[test]
fn a_symlinked_manifest_is_not_read() {
    let root = tempfile::tempdir().unwrap();
    let dir = repo(root.path(), "app");
    let secret = root.path().join("secret.toml");
    fs::write(&secret, "[package]\nname = \"secret\"\n").unwrap();
    std::os::unix::fs::symlink(&secret, dir.join("Cargo.toml")).unwrap();

    let report = run(&[root.path()]);

    let found = &report.repositories[0];
    assert!(found.cargo.is_none());
    assert!(
        found.notes.iter().any(|n| n.contains("symlink")),
        "{:?}",
        found.notes
    );
}

#[cfg(unix)]
#[test]
fn a_git_symlink_does_not_make_a_repository() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let dir = root.path().join("fake");
    fs::create_dir(&dir).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), dir.join(".git")).unwrap();

    let report = run(&[root.path()]);

    assert!(report.repositories.is_empty(), "{:?}", paths(&report));
}

#[cfg(unix)]
#[test]
fn an_unreadable_root_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let locked = root.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    // A root user reads any folder, so the check has nothing to show there.
    if fs::read_dir(&locked).is_ok() {
        return;
    }

    let error = scan_with(&[locked.clone()], Bounds::DEFAULT);

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let error = error.unwrap_err();
    assert!(format!("{error:#}").contains("read access"), "{error:#}");
}

#[cfg(unix)]
#[test]
fn a_fifo_manifest_is_a_note_and_does_not_block() {
    let root = tempfile::tempdir().unwrap();
    let dir = repo(root.path(), "app");
    let made = std::process::Command::new("mkfifo")
        .arg(dir.join("Cargo.toml"))
        .status()
        .unwrap();
    assert!(made.success());

    let report = run(&[root.path()]);

    let found = &report.repositories[0];
    assert!(found.cargo.is_none());
    assert!(
        found.notes.iter().any(|n| n.contains("not a regular file")),
        "{:?}",
        found.notes
    );
}

#[cfg(unix)]
#[test]
fn a_linked_github_folder_is_not_listed() {
    let root = tempfile::tempdir().unwrap();
    let dir = repo(root.path(), "app");
    let elsewhere = tempfile::tempdir().unwrap();
    fs::create_dir_all(elsewhere.path().join("workflows")).unwrap();
    fs::write(elsewhere.path().join("workflows/secret.yml"), "x").unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), dir.join(".github")).unwrap();

    let report = run(&[root.path()]);

    let found = &report.repositories[0];
    assert!(found.workflows.is_empty(), "{:?}", found.workflows);
    assert!(
        found.notes.iter().any(|n| n.contains(".github")),
        "{:?}",
        found.notes
    );
}

#[test]
fn the_filesystem_root_home_and_folders_above_home_are_refused() {
    let base = tempfile::tempdir().unwrap();
    let home = base.path().join("users/me");
    fs::create_dir_all(&home).unwrap();
    for root in [PathBuf::from("/"), home.clone(), base.path().join("users")] {
        let error = scan(&[root.clone()], &home, Bounds::DEFAULT).unwrap_err();
        assert!(
            format!("{error:#}").contains("narrower"),
            "{}: {error:#}",
            root.display()
        );
    }
    let inside = home.join("git");
    fs::create_dir(&inside).unwrap();
    assert!(scan(&[inside], &home, Bounds::DEFAULT).is_ok());
}

#[test]
fn a_home_folder_that_cannot_be_resolved_fails_closed() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("git");
    fs::create_dir(&root).unwrap();
    let error = scan(&[root], &base.path().join("absent"), Bounds::DEFAULT).unwrap_err();
    assert!(format!("{error:#}").contains("HOME"), "{error:#}");
}
