//! The per-user TMPDIR root that Claude Code creates under `state/tmp` (#1379).
//!
//! The root is never removed, because its mtime does not show use. Its entries are judged one by
//! one, with the same in-use check as the rest of the sweep.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{Scan, judge_entry, record_failure};

/// Judge the entries one level down inside Claude Code's `claude-<uid>` root. The root itself is
/// never a candidate.
pub(super) fn scan_claude_root(
    claude_root: &Path,
    older_than: Duration,
    in_use: &[PathBuf],
    now: SystemTime,
    scan: &mut Scan,
) {
    let entries = match crate::paths::read_dir_optional(claude_root) {
        Ok(Some(entries)) => entries,
        Ok(None) => return,
        Err(e) => {
            record_failure(claude_root, &e, scan);
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                record_failure(claude_root, &e, scan);
                continue;
            }
        };
        let path = claude_root.join(entry.file_name());
        if in_use.iter().any(|used| used.starts_with(&path)) {
            scan.kept += 1;
            continue;
        }
        match entry.file_type() {
            Ok(file_type) => judge_entry(&path, file_type.is_dir(), older_than, now, scan),
            Err(e) => record_failure(&path, &e, scan),
        }
    }
}
/// Whether `name` is `claude-<uid>`, the per-user TMPDIR root that Claude Code creates.
pub(super) fn is_claude_root(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix("claude-"))
        .is_some_and(|uid| !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()))
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::fs::{self, File};
    use std::time::SystemTime;

    use crate::materialize::tmp_sweep::sweep;
    use crate::materialize::tmp_sweep::tests::{DAY, age_tree, old};

    #[test]
    fn a_claude_code_root_is_kept_even_when_old() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("claude-501");
        fs::create_dir(&claude).unwrap();
        age_tree(&claude, old());

        let report = sweep(root.path(), DAY * 7, &[], false).unwrap();

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

        let report = sweep(root.path(), DAY * 7, &[], false).unwrap();

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

        let report = sweep(root.path(), DAY * 7, std::slice::from_ref(&cwd_file), false).unwrap();

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

        let report = sweep(root.path(), DAY * 7, &[], false).unwrap();

        assert!(report.removed.is_empty());
        assert!(future.exists());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod name_tests {
    use proptest::prelude::*;

    use super::is_claude_root;

    #[test]
    fn only_a_claude_prefix_with_digits_is_a_root() {
        for yes in ["claude-501", "claude-0"] {
            assert!(is_claude_root(std::ffi::OsStr::new(yes)), "{yes}");
        }
        for no in [
            "claude-",
            "claude-x",
            "claude-5a",
            "claude501",
            "Claude-501",
            "shell-501",
        ] {
            assert!(!is_claude_root(std::ffi::OsStr::new(no)), "{no}");
        }
    }

    proptest! {
        /// A name without the `claude-` prefix is never a root, so the sweep judges it normally.
        #[test]
        fn a_name_without_the_prefix_is_never_a_root(name in "[a-z0-9-]{0,12}") {
            prop_assume!(!name.starts_with("claude-"));

            prop_assert!(!is_claude_root(std::ffi::OsStr::new(&name)));
        }
    }
}
