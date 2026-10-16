//! Age sweep of the state temp folder, `state/tmp` (#2814).
//!
//! `TMPDIR` points at this folder, so every tool that honors it writes here. Nothing else
//! removes what those tools leave behind. An entry is removed when its newest mtime is older
//! than the retention, and no process holds a path inside it open.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::Context;

use crate::materialize::cache::{
    GcReport, gc_remove_dir, gc_remove_link, is_expired, newest_mtime,
};

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
        // Claude Code keeps its per-user TMPDIR root here. Its mtime does not show use, so an age
        // sweep would delete the root of a live shell (#1379).
        if is_dir && is_claude_root(&name) {
            report.kept += 1;
            continue;
        }
        let modified = match entry_mtime(&path, is_dir) {
            Ok(modified) => modified,
            Err(e) => {
                record_failure(&path, &e, &mut report);
                continue;
            }
        };
        // A future mtime means the clock moved. Keep the entry, because a removal on a bad
        // clock is not reversible.
        if modified > now || !is_expired(now, modified, older_than) {
            report.kept += 1;
        } else if is_dir {
            gc_remove_dir(&path, &mut report);
        } else {
            gc_remove_link(&path, &mut report);
        }
    }
    Ok(report)
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

/// Paths that a process holds open or uses as its working directory.
///
/// # Errors
/// Fails when `lsof` cannot run or does not exit 0. A sweep without a complete list could
/// remove an entry that a live process uses, so the sweep stops instead.
#[cfg(unix)]
pub fn open_paths() -> anyhow::Result<Vec<PathBuf>> {
    lsof_paths("lsof")
}

/// The sweep needs lsof to find the entries in use, and lsof is a unix tool.
///
/// # Errors
/// Always fails, because the in-use list cannot be built on this platform.
#[cfg(not(unix))]
pub fn open_paths() -> anyhow::Result<Vec<PathBuf>> {
    anyhow::bail!("the state/tmp sweep needs lsof, which runs on unix only; the sweep stopped")
}

/// Run `program -F n` and read the paths it names. `program` is a parameter so a test can
/// name a binary that fails.
#[cfg(unix)]
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
    // lsof exits 1 on any error, and its list is then incomplete. Only exit 0 counts.
    if output.status.code() != Some(0) {
        anyhow::bail!(
            "`{program}` exited with {}; the state/tmp sweep stopped. Fix: check that `{program}` runs",
            output.status
        );
    }
    // lsof reports partial results on stderr, for example for a mount it cannot stat. Those
    // results are routine on some hosts, so they are logged, not fatal.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        tracing::warn!(
            stderr = %stderr.trim(),
            "lsof reported a problem; the in-use list may be incomplete"
        );
    }
    Ok(parse_lsof_names(&output.stdout))
}

/// The name fields of `lsof -F n` output that are absolute paths. Socket and pipe names do not
/// start with `/`, so they drop out. Names are kept as bytes, so a non-UTF-8 name still matches.
#[cfg(unix)]
fn parse_lsof_names(output: &[u8]) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    output
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.strip_prefix(b"n"))
        .filter(|name| name.starts_with(b"/"))
        .map(|name| PathBuf::from(std::ffi::OsStr::from_bytes(name)))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::sweep;

    #[cfg(unix)]
    use super::{lsof_paths, parse_lsof_names};

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

    #[cfg(unix)]
    #[test]
    fn parse_keeps_only_the_absolute_path_names() {
        let out = b"p123\nfcwd\nn/work/a\nnTCP *:8080\np9\nn/work/b (deleted)\n";
        let paths = parse_lsof_names(out);
        assert_eq!(
            paths,
            vec![
                std::path::PathBuf::from("/work/a"),
                std::path::PathBuf::from("/work/b (deleted)"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn parse_keeps_a_non_utf8_name_byte_for_byte() {
        use std::os::unix::ffi::OsStrExt;
        let paths = parse_lsof_names(b"n/tmp/\xff\n");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].as_os_str().as_bytes(), b"/tmp/\xff");
    }

    #[cfg(unix)]
    #[test]
    fn parse_yields_one_path_per_absolute_name_line() {
        use proptest::prelude::*;
        proptest!(|(lines in proptest::collection::vec("[pfn/ a-z]{0,10}", 0..12))| {
            let text = lines.join("\n");
            let expected: Vec<String> = lines
                .iter()
                .filter(|line| line.starts_with("n/"))
                .map(|line| line[1..].to_owned())
                .collect();
            let got: Vec<String> = parse_lsof_names(text.as_bytes())
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            prop_assert_eq!(got, expected);
        });
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn an_lsof_exit_other_than_zero_is_an_error_even_with_empty_output() {
        // `false` exits 1 and prints nothing. An exit 1 is a failure, not an empty list.
        let err = lsof_paths("false").unwrap_err();
        assert!(format!("{err:#}").contains("exited with"), "{err:#}");
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
