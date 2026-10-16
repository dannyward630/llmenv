//! Age sweep of the state temp folder, `state/tmp` (#2814).
//!
//! `TMPDIR` points at this folder, so every tool that honors it writes here. Nothing else
//! removes what those tools leave behind. An entry is removed when its newest mtime is older
//! than the retention, and no process holds a path inside it open.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::Context;

use crate::materialize::cache::{GcReport, gc_remove_dir, is_expired, newest_mtime};

/// Remove the entries of `tmp_root` that are older than `older_than` and not in use.
///
/// `in_use` lists paths that a process holds open. An entry that contains one of them is kept,
/// because removing it would break a live process.
pub fn sweep(
    tmp_root: &Path,
    older_than: Duration,
    in_use: &[PathBuf],
) -> anyhow::Result<GcReport> {
    let mut report = GcReport::default();
    let Some(entries) = crate::paths::read_dir_optional(tmp_root)? else {
        return Ok(report);
    };
    let now = SystemTime::now();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", tmp_root.display()))?;
        let path = entry.path();
        let is_dir = entry
            .file_type()
            .with_context(|| format!("stat {}", path.display()))?
            .is_dir();
        if in_use.iter().any(|used| used.starts_with(&path)) {
            report.kept += 1;
            continue;
        }
        // A symlink is judged by its own mtime and removed as a link, never followed.
        let modified = if is_dir {
            newest_mtime(&path)?
        } else {
            std::fs::symlink_metadata(&path)
                .and_then(|m| m.modified())
                .with_context(|| format!("stat {}", path.display()))?
        };
        if !is_expired(now, modified, older_than) {
            report.kept += 1;
        } else if is_dir {
            gc_remove_dir(&path, &mut report);
        } else {
            remove_file_entry(&path, &mut report);
        }
    }
    Ok(report)
}

/// Remove a file or a symlink for [`sweep`], recording rather than propagating a failure.
/// One undeletable entry must not stop the sweep of the entries after it (#1372).
fn remove_file_entry(path: &Path, report: &mut GcReport) {
    match std::fs::remove_file(path) {
        Ok(()) => report.removed.push(path.to_path_buf()),
        Err(e) => {
            tracing::error!(
                path = %path.display(),
                error = %e,
                "state/tmp sweep: failed to remove entry; skipping"
            );
            report.failed.push(path.to_path_buf());
        }
    }
}

/// Sweep `<state_dir>/tmp` with the retention `older_than` (#2814).
///
/// # Errors
/// Fails when the in-use list cannot be read. See [`open_paths`].
pub fn sweep_state_tmp(state_dir: &Path, older_than: Duration) -> anyhow::Result<GcReport> {
    let in_use = open_paths()?;
    sweep(&state_dir.join("tmp"), older_than, &in_use)
}

/// Paths that a process holds open or uses as its working directory.
///
/// # Errors
/// Fails when `lsof` cannot run. A sweep without this list could remove an entry that a live
/// process uses, so the sweep stops instead.
pub fn open_paths() -> anyhow::Result<Vec<PathBuf>> {
    lsof_paths("lsof")
}

/// Run `program -F n` and read the paths it names. `program` is a parameter so a test can
/// name a missing binary.
fn lsof_paths(program: &str) -> anyhow::Result<Vec<PathBuf>> {
    let output = Command::new(program)
        .args(["-F", "n"])
        .output()
        .with_context(|| {
            format!(
                "running `{program}` to find entries in use; the state/tmp sweep stopped. \
                 Fix: install lsof, then run `llmenv doctor --gc` again"
            )
        })?;
    // lsof exits 1 when it has nothing to report. Any other failure means its list is not
    // complete, so the sweep must not run on it.
    if !matches!(output.status.code(), Some(0 | 1)) {
        anyhow::bail!(
            "`{program}` failed ({}); the state/tmp sweep stopped. \
             Fix: check that `{program}` runs",
            output.status
        );
    }
    Ok(parse_lsof_names(&String::from_utf8_lossy(&output.stdout)))
}

/// The name fields of `lsof -F n` output that are absolute paths. Socket and pipe names do not
/// start with `/`, so they drop out.
fn parse_lsof_names(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .filter(|name| name.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::{lsof_paths, parse_lsof_names, sweep};

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
    fn parse_keeps_only_the_absolute_path_names() {
        let out = "p123\nfcwd\nn/work/a\nnTCP *:8080\np9\nn/work/b (deleted)\n";
        let paths = parse_lsof_names(out);
        assert_eq!(
            paths,
            vec![
                std::path::PathBuf::from("/work/a"),
                std::path::PathBuf::from("/work/b (deleted)"),
            ]
        );
    }

    #[test]
    fn a_missing_lsof_is_an_error_that_names_the_fix() {
        let err = lsof_paths("llmenv-no-such-lsof-binary").unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("Fix:"), "no fix named: {text}");
        assert!(
            text.contains("state/tmp sweep stopped"),
            "no effect named: {text}"
        );
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

        assert_eq!(report.removed, vec![link.clone()]);
        assert!(link.symlink_metadata().is_err(), "the link survived");
        assert!(target.join("data").exists(), "the link target was followed");
    }
}
