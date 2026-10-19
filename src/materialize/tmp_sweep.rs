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
use crate::materialize::status_data::dir_size;

mod claude_root;

use claude_root::{is_claude_root, scan_claude_root};

/// One entry that the scan found old enough to remove, and not in use.
struct Stale {
    path: PathBuf,
    is_dir: bool,
}

/// What a scan of `state/tmp` found. Nothing is removed by a scan.
#[derive(Default)]
struct Scan {
    stale: Vec<Stale>,
    kept: usize,
    failed: Vec<PathBuf>,
}

/// The stale entries of `state/tmp`: how many, and how many bytes they hold.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StaleSummary {
    pub count: usize,
    pub bytes: u64,
    /// Entries the scan could not read or judge. They are not in `count`.
    pub unreadable: usize,
}

/// Remove the entries of `tmp_root` that are older than `older_than` and not in use.
///
/// `in_use` lists paths that a process holds open. An entry that contains one of them is kept,
/// because removing it would break a live process. lsof reports resolved paths, so the root is
/// resolved too. Otherwise a symlink on the way to the root hides every match.
///
/// A failure on one entry is recorded and the sweep goes on, so one unreadable entry does not
/// block the others (#1372).
///
/// With `dry_run`, the report lists the same entries and nothing is removed (#2917).
pub fn sweep(
    tmp_root: &Path,
    older_than: Duration,
    in_use: &[PathBuf],
    dry_run: bool,
) -> anyhow::Result<GcReport> {
    let scan = scan(tmp_root, older_than, in_use)?;
    let mut report = GcReport {
        kept: scan.kept,
        failed: scan.failed,
        ..GcReport::new(dry_run)
    };
    for stale in &scan.stale {
        if stale.is_dir {
            gc_remove_dir(&stale.path, &mut report);
        } else {
            gc_remove_link(&stale.path, &mut report);
        }
    }
    Ok(report)
}

/// Count and size the entries that [`sweep`] would remove, without removing any.
///
/// # Errors
/// Fails when the in-use list cannot be read. See [`open_paths`].
pub fn stale_summary(state_dir: &Path, older_than: Duration) -> anyhow::Result<StaleSummary> {
    let in_use = open_paths()?;
    stale_summary_in(&state_dir.join("tmp"), older_than, &in_use)
}

/// The summary for `tmp_root`, with the in-use list given by the caller.
///
/// # Errors
/// Fails when `tmp_root` cannot be read or resolved.
pub fn stale_summary_in(
    tmp_root: &Path,
    older_than: Duration,
    in_use: &[PathBuf],
) -> anyhow::Result<StaleSummary> {
    let scan = scan(tmp_root, older_than, in_use)?;
    Ok(StaleSummary {
        count: scan.stale.len(),
        bytes: scan.stale.iter().map(|s| dir_size(&s.path)).sum(),
        unreadable: scan.failed.len(),
    })
}

/// Walk `tmp_root` and sort every entry into stale, kept, or failed. The walk changes nothing.
fn scan(tmp_root: &Path, older_than: Duration, in_use: &[PathBuf]) -> anyhow::Result<Scan> {
    let mut scan = Scan::default();
    let Some(entries) = crate::paths::read_dir_optional(tmp_root)? else {
        return Ok(scan);
    };
    let root = std::fs::canonicalize(tmp_root)
        .with_context(|| format!("resolving {}", tmp_root.display()))?;
    let now = SystemTime::now();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                record_failure(&root, &e, &mut scan);
                continue;
            }
        };
        let name = entry.file_name();
        let path = root.join(&name);
        if in_use.iter().any(|used| used.starts_with(&path)) {
            scan.kept += 1;
            continue;
        }
        let is_dir = match entry.file_type() {
            Ok(file_type) => file_type.is_dir(),
            Err(e) => {
                record_failure(&path, &e, &mut scan);
                continue;
            }
        };
        // The per-user TMPDIR root is not judged by its own mtime, because that does not show use
        // (#1379). Its entries are judged one by one instead.
        if is_dir && is_claude_root(&name) {
            scan_claude_root(&path, older_than, in_use, now, &mut scan);
            continue;
        }
        judge_entry(&path, is_dir, older_than, now, &mut scan);
    }
    Ok(scan)
}

/// Mark one entry stale when its newest mtime is older than `older_than`. Keep it otherwise.
fn judge_entry(path: &Path, is_dir: bool, older_than: Duration, now: SystemTime, scan: &mut Scan) {
    let modified = match entry_mtime(path, is_dir) {
        Ok(modified) => modified,
        Err(e) => {
            record_failure(path, &e, scan);
            return;
        }
    };
    // A future mtime means the clock moved. Keep the entry, because a removal on a bad
    // clock is not reversible.
    if modified > now || !is_expired(now, modified, older_than) {
        scan.kept += 1;
    } else {
        scan.stale.push(Stale {
            path: path.to_path_buf(),
            is_dir,
        });
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

/// Record an entry the sweep could not read or judge. The entry stays, and the next run retries.
fn record_failure(path: &Path, error: impl std::fmt::Display, scan: &mut Scan) {
    tracing::error!(
        path = %path.display(),
        error = %error,
        "state/tmp sweep: cannot read entry; skipping"
    );
    scan.failed.push(path.to_path_buf());
}

/// Sweep `<state_dir>/tmp` with the retention `older_than` (#2814).
///
/// # Errors
/// Fails when the in-use list cannot be read. See [`open_paths`].
pub fn sweep_state_tmp(
    state_dir: &Path,
    older_than: Duration,
    dry_run: bool,
) -> anyhow::Result<GcReport> {
    let in_use = open_paths()?;
    sweep(&state_dir.join("tmp"), older_than, &in_use, dry_run)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::{StaleSummary, stale_summary_in, sweep};

    pub(super) const DAY: Duration = Duration::from_secs(86_400);

    /// Set the mtime of `path` and of everything under it, so the newest mtime is `when`.
    pub(super) fn age_tree(path: &Path, when: SystemTime) {
        if path.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                age_tree(&entry.unwrap().path(), when);
            }
        }
        File::open(path).unwrap().set_modified(when).unwrap();
    }

    pub(super) fn old() -> SystemTime {
        SystemTime::now() - 30 * DAY
    }

    #[test]
    fn a_dry_run_lists_the_old_entries_and_removes_none() {
        let root = tempfile::tempdir().unwrap();
        let old_dir = root.path().join("cargo-old");
        fs::create_dir(&old_dir).unwrap();
        fs::write(old_dir.join("artifact"), b"x").unwrap();
        age_tree(&old_dir, old());
        let old_file = root.path().join("scratch.txt");
        fs::write(&old_file, b"y").unwrap();
        age_tree(&old_file, old());
        fs::create_dir(root.path().join("fresh")).unwrap();

        let report = sweep(root.path(), DAY * 7, &[], true).unwrap();

        let mut listed = report.removed.clone();
        listed.sort();
        let mut expected = vec![old_dir.clone(), old_file.clone()];
        expected.sort();
        assert_eq!(listed, expected);
        assert_eq!(report.kept, 1);
        assert!(old_dir.exists() && old_file.exists());
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

        let report = sweep(root.path(), DAY * 7, &[], false).unwrap();

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

        let report = sweep(root.path(), DAY * 7, std::slice::from_ref(&held), false).unwrap();

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
        let report = sweep(&link, DAY * 7, std::slice::from_ref(&held), false).unwrap();

        assert!(
            report.removed.is_empty(),
            "removed a busy entry: {:?}",
            report.removed
        );
        assert!(held.exists());
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

        let report = sweep(root.path(), DAY * 7, &[], false).unwrap();

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

        let report = sweep(root.path(), DAY * 7, &[], false).unwrap();

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

        let report = sweep(&tmp, Duration::ZERO, &[], false).unwrap();

        assert_eq!(
            report.removed,
            vec![tmp.canonicalize().unwrap().join("link")]
        );
        assert!(link.symlink_metadata().is_err(), "the link survived");
        assert!(target.join("data").exists(), "the link target was followed");
    }

    #[test]
    fn the_summary_counts_and_sizes_the_stale_entries_and_removes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let old_dir = root.path().join("cargo-old");
        fs::create_dir(&old_dir).unwrap();
        fs::write(old_dir.join("a"), [0_u8; 300]).unwrap();
        fs::write(old_dir.join("b"), [0_u8; 200]).unwrap();
        age_tree(&old_dir, old());
        let fresh = root.path().join("fresh");
        fs::create_dir(&fresh).unwrap();
        fs::write(fresh.join("x"), [0_u8; 999]).unwrap();

        let summary = stale_summary_in(root.path(), DAY * 7, &[]).unwrap();

        assert_eq!(
            summary,
            StaleSummary {
                count: 1,
                bytes: 500,
                unreadable: 0
            }
        );
        assert!(old_dir.join("a").exists(), "the summary removed an entry");
    }

    #[test]
    fn the_summary_skips_a_claude_code_root_and_an_entry_in_use() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("claude-501");
        fs::create_dir(&claude).unwrap();
        age_tree(&claude, old());
        let busy = root.path().join("cargo-busy");
        fs::create_dir(&busy).unwrap();
        age_tree(&busy, old());

        let summary = stale_summary_in(root.path(), DAY * 7, std::slice::from_ref(&busy)).unwrap();

        assert_eq!(
            summary,
            StaleSummary {
                count: 0,
                bytes: 0,
                unreadable: 0
            }
        );
    }

    #[test]
    fn the_summary_of_a_missing_tmp_root_is_empty() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("no-such-tmp");

        let summary = stale_summary_in(&missing, DAY * 7, &[]).unwrap();

        assert_eq!(
            summary,
            StaleSummary {
                count: 0,
                bytes: 0,
                unreadable: 0
            }
        );
    }
}
