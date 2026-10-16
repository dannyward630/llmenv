//! Age sweep of the state temp folder, `state/tmp` (#2814).
//!
//! `TMPDIR` points at this folder, so every tool that honors it writes here. Nothing else
//! removes what those tools leave behind. An entry is removed when its newest mtime is older
//! than the retention, and no process holds a path inside it open.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Context;

use crate::materialize::cache::{
    GcReport, gc_remove_dir, gc_remove_link, is_expired, newest_mtime,
};
use crate::materialize::open_files::open_paths;

/// Remove the entries of `tmp_root` that are older than `older_than` and not in use.
///
/// `in_use` lists paths that a process holds open. An entry that contains one of them is kept,
/// because removing it would break a live process. lsof reports resolved paths, so the root is
/// resolved too. Otherwise a symlink on the way to the root hides every match.
///
/// A failure on one entry is recorded and the sweep goes on, so one unreadable entry does not
/// block the others (#1372).
pub fn sweep(
    tmp_root: &Path,
    older_than: Duration,
    in_use: &[PathBuf],
) -> anyhow::Result<GcReport> {
    let mut report = GcReport::default();
    let Some(entries) = crate::paths::read_dir_optional(tmp_root)? else {
        return Ok(report);
    };
    let root = std::fs::canonicalize(tmp_root)
        .with_context(|| format!("resolving {}", tmp_root.display()))?;
    let now = SystemTime::now();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", root.display()))?;
        let name = entry.file_name();
        let path = root.join(&name);
        if in_use.iter().any(|used| used.starts_with(&path)) {
            report.kept += 1;
            continue;
        }
        let is_dir = match entry.file_type() {
            Ok(file_type) => file_type.is_dir(),
            Err(e) => {
                record_failure(&path, &e, &mut report);
                continue;
            }
        };
        // The per-user TMPDIR root is not judged by its own mtime, because that does not show use
        // (#1379). Its entries are judged one by one instead.
        if is_dir && is_claude_root(&name) {
            sweep_claude_root(&path, older_than, in_use, now, &mut report);
            continue;
        }
        judge_entry(&path, is_dir, older_than, now, &mut report);
    }
    Ok(report)
}

/// Remove the entries one level down inside Claude Code's `claude-<uid>` root that are older
/// than `older_than` and not in use. The root itself is never removed.
fn sweep_claude_root(
    claude_root: &Path,
    older_than: Duration,
    in_use: &[PathBuf],
    now: SystemTime,
    report: &mut GcReport,
) {
    let entries = match crate::paths::read_dir_optional(claude_root) {
        Ok(Some(entries)) => entries,
        Ok(None) => return,
        Err(e) => {
            record_failure(claude_root, &e, report);
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                record_failure(claude_root, &e, report);
                continue;
            }
        };
        let path = claude_root.join(entry.file_name());
        if in_use.iter().any(|used| used.starts_with(&path)) {
            report.kept += 1;
            continue;
        }
        match entry.file_type() {
            Ok(file_type) => judge_entry(&path, file_type.is_dir(), older_than, now, report),
            Err(e) => record_failure(&path, &e, report),
        }
    }
}

/// Remove one entry when its newest mtime is older than `older_than`. Keep it otherwise.
fn judge_entry(
    path: &Path,
    is_dir: bool,
    older_than: Duration,
    now: SystemTime,
    report: &mut GcReport,
) {
    let modified = match entry_mtime(path, is_dir) {
        Ok(modified) => modified,
        Err(e) => {
            record_failure(path, &e, report);
            return;
        }
    };
    // A future mtime means the clock moved. Keep the entry, because a removal on a bad
    // clock is not reversible.
    if modified > now || !is_expired(now, modified, older_than) {
        report.kept += 1;
    } else if is_dir {
        gc_remove_dir(path, report);
    } else {
        gc_remove_link(path, report);
    }
}

/// The newest mtime of an entry. A symlink is judged by its own mtime and never followed.
fn entry_mtime(path: &Path, is_dir: bool) -> anyhow::Result<SystemTime> {
    if is_dir {
        return newest_mtime(path);
    }
    std::fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .with_context(|| format!("stat {}", path.display()))
}

/// Whether `name` is `claude-<uid>`, the per-user TMPDIR root that Claude Code creates.
fn is_claude_root(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix("claude-"))
        .is_some_and(|uid| !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()))
}

/// Record an entry the sweep could not read or judge. The entry stays, and the next run retries.
fn record_failure(path: &Path, error: impl std::fmt::Display, report: &mut GcReport) {
    tracing::error!(
        path = %path.display(),
        error = %error,
        "state/tmp sweep: cannot read entry; skipping"
    );
    report.failed.push(path.to_path_buf());
}

/// Sweep `<state_dir>/tmp` with the retention `older_than` (#2814).
///
/// # Errors
/// Fails when the in-use list cannot be read. See [`open_paths`].
pub fn sweep_state_tmp(state_dir: &Path, older_than: Duration) -> anyhow::Result<GcReport> {
    let in_use = open_paths()?;
    sweep(&state_dir.join("tmp"), older_than, &in_use)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::sweep;

    const DAY: Duration = Duration::from_secs(86_400);

    /// Set the mtime of `path` and of everything under it, so the newest mtime is `when`.
    fn age_tree(path: &Path, when: SystemTime) {
        if path.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                age_tree(&entry.unwrap().path(), when);
            }
        }
        File::open(path).unwrap().set_modified(when).unwrap();
    }

    fn old() -> SystemTime {
        SystemTime::now() - 30 * DAY
    }

    #[test]
    fn an_old_directory_and_an_old_file_are_removed_and_a_fresh_one_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let old_dir = root.path().join("cargo-old");
        fs::create_dir(&old_dir).unwrap();
        fs::write(old_dir.join("artifact"), b"x").unwrap();
        age_tree(&old_dir, old());
        let old_file = root.path().join("scratch.txt");
        fs::write(&old_file, b"y").unwrap();
        age_tree(&old_file, old());
        let fresh = root.path().join("fresh");
        fs::create_dir(&fresh).unwrap();

        let report = sweep(root.path(), DAY * 7, &[]).unwrap();

        assert_eq!(report.removed.len(), 2, "removed: {:?}", report.removed);
        assert_eq!(report.kept, 1);
        assert!(!old_dir.exists());
        assert!(!old_file.exists());
        assert!(fresh.exists());
    }

    #[test]
    fn an_old_entry_with_a_path_in_use_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let busy = root.path().join("cargo-busy");
        fs::create_dir(&busy).unwrap();
        let held = busy.join("open.log");
        fs::write(&held, b"z").unwrap();
        age_tree(&busy, old());

        let report = sweep(root.path(), DAY * 7, std::slice::from_ref(&held)).unwrap();

        assert!(
            report.removed.is_empty(),
            "removed a busy entry: {:?}",
            report.removed
        );
        assert_eq!(report.kept, 1);
        assert!(held.exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_in_use_path_matches_when_the_root_is_reached_through_a_symlink() {
        let real = tempfile::tempdir().unwrap();
        let busy = real.path().join("cargo-busy");
        fs::create_dir(&busy).unwrap();
        let held = busy.join("open.log");
        fs::write(&held, b"z").unwrap();
        age_tree(&busy, old());
        let linked_dir = tempfile::tempdir().unwrap();
        let link = linked_dir.path().join("tmp-link");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();

        // lsof reports the resolved path, so the in-use list holds it, not the link path.
        let report = sweep(&link, DAY * 7, std::slice::from_ref(&held)).unwrap();

        assert!(
            report.removed.is_empty(),
            "removed a busy entry: {:?}",
            report.removed
        );
        assert!(held.exists());
    }

    #[test]
    fn a_claude_code_root_is_kept_even_when_old() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("claude-501");
        fs::create_dir(&claude).unwrap();
        age_tree(&claude, old());

        let report = sweep(root.path(), DAY * 7, &[]).unwrap();

        assert!(report.removed.is_empty());
        assert!(claude.exists(), "the per-user TMPDIR root was removed");
    }

    #[test]
    fn an_old_entry_inside_a_claude_code_root_is_removed_and_a_fresh_one_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("claude-501");
        fs::create_dir(&claude).unwrap();
        let old_child = claude.join("shell-old");
        fs::create_dir(&old_child).unwrap();
        fs::write(old_child.join("snapshot"), b"s").unwrap();
        age_tree(&old_child, old());
        let fresh_child = claude.join("shell-fresh");
        fs::create_dir(&fresh_child).unwrap();

        let report = sweep(root.path(), DAY * 7, &[]).unwrap();

        assert_eq!(report.removed.len(), 1, "removed: {:?}", report.removed);
        assert!(
            !old_child.exists(),
            "the old entry inside the root survived"
        );
        assert!(
            fresh_child.exists(),
            "a fresh entry inside the root was removed"
        );
        assert!(claude.exists(), "the per-user TMPDIR root was removed");
    }

    #[test]
    fn an_old_entry_inside_a_claude_code_root_with_a_path_in_use_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("claude-501");
        fs::create_dir(&claude).unwrap();
        let busy = claude.join("shell-busy");
        fs::create_dir(&busy).unwrap();
        let cwd_file = busy.join("cwd.txt");
        fs::write(&cwd_file, b"c").unwrap();
        age_tree(&busy, old());

        let report = sweep(root.path(), DAY * 7, std::slice::from_ref(&cwd_file)).unwrap();

        assert!(
            report.removed.is_empty(),
            "removed a busy entry: {:?}",
            report.removed
        );
        assert!(cwd_file.exists());
    }

    #[test]
    fn an_entry_with_a_future_mtime_inside_a_claude_code_root_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("claude-501");
        fs::create_dir(&claude).unwrap();
        let future = claude.join("future.txt");
        fs::write(&future, b"f").unwrap();
        File::open(&future)
            .unwrap()
            .set_modified(SystemTime::now() + DAY)
            .unwrap();

        let report = sweep(root.path(), DAY * 7, &[]).unwrap();

        assert!(report.removed.is_empty());
        assert!(future.exists());
    }

    #[test]
    fn an_entry_with_a_future_mtime_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let future = root.path().join("future.txt");
        fs::write(&future, b"f").unwrap();
        File::open(&future)
            .unwrap()
            .set_modified(SystemTime::now() + DAY)
            .unwrap();

        let report = sweep(root.path(), DAY * 7, &[]).unwrap();

        assert!(report.removed.is_empty());
        assert!(future.exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_entry_is_recorded_and_the_rest_are_still_swept() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let locked = root.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("inner"), b"i").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let old_file = root.path().join("old.txt");
        fs::write(&old_file, b"o").unwrap();
        age_tree(&old_file, old());

        let report = sweep(root.path(), DAY * 7, &[]).unwrap();

        // Restore the mode before any assertion, so the tempdir can be removed.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            report.failed,
            vec![root.path().canonicalize().unwrap().join("locked")]
        );
        assert_eq!(report.removed.len(), 1, "removed: {:?}", report.removed);
        assert!(!old_file.exists());
    }

    /// The link is new, so only a zero retention makes it expired. The project's other GC
    /// tests use the same zero retention.
    #[cfg(unix)]
    #[test]
    fn an_expired_symlink_is_removed_and_its_target_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("keep-me");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("data"), b"d").unwrap();
        let tmp = root.path().join("tmp");
        fs::create_dir(&tmp).unwrap();
        let link = tmp.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let report = sweep(&tmp, Duration::ZERO, &[]).unwrap();

        assert_eq!(
            report.removed,
            vec![tmp.canonicalize().unwrap().join("link")]
        );
        assert!(link.symlink_metadata().is_err(), "the link survived");
        assert!(target.join("data").exists(), "the link target was followed");
    }
}
