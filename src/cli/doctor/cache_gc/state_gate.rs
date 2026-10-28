//! Decides whether the state sweeps can remove anything, so the lsof in-use list is fetched only
//! when a sweep might need it (#3039).

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::materialize::tmp_sweep;

/// Whether a sweep of `state/tmp` or `state/codebase-memory` could remove an entry. Both sweeps
/// remove only entries older than `retention`, so `false` means the in-use list is not needed.
/// An entry that cannot be read counts as possibly stale.
pub(super) fn state_may_have_stale_entries(state_dir: &Path, retention: Duration) -> bool {
    let tmp = state_dir.join("tmp");
    let tmp_may_be_stale = match tmp_sweep::stale_summary_in(&tmp, retention, &[]) {
        Ok(summary) => summary.count > 0 || summary.unreadable > 0,
        Err(e) => {
            tracing::error!(
                "state GC cannot scan {}, so lsof runs: {e:#}",
                tmp.display()
            );
            true
        }
    };
    tmp_may_be_stale || cbm_has_entry_older_than(&state_dir.join("codebase-memory"), retention)
}

/// Whether `dir` holds an entry last written at or before `now - retention`. A database is
/// removed only when the newest file of its unit is that old, so every removable unit has such
/// an entry. An unreadable entry counts as old, and it is logged.
fn cbm_has_entry_older_than(dir: &Path, retention: Duration) -> bool {
    let Some(cutoff) = SystemTime::now().checked_sub(retention) else {
        tracing::error!("state GC cannot compute the cutoff, so lsof runs");
        return true;
    };
    let mut entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return false,
        Err(e) => {
            tracing::error!("state GC cannot read {}, so lsof runs: {e}", dir.display());
            return true;
        }
    };
    entries.any(|entry| {
        let modified = entry
            .and_then(|e| e.metadata())
            .and_then(|meta| meta.modified());
        match modified {
            Ok(modified) => modified <= cutoff,
            Err(e) => {
                tracing::error!(
                    "state GC cannot read an entry of {}, so lsof runs: {e}",
                    dir.display()
                );
                true
            }
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs::File;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::state_may_have_stale_entries;

    const DAY: Duration = Duration::from_secs(86_400);

    fn age(path: &Path) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - 30 * DAY)
            .unwrap();
    }

    #[test]
    fn a_state_folder_with_only_fresh_entries_needs_no_in_use_list() {
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir(state.path().join("tmp")).unwrap();
        std::fs::write(state.path().join("tmp").join("fresh"), b"x").unwrap();
        std::fs::create_dir(state.path().join("codebase-memory")).unwrap();
        std::fs::write(state.path().join("codebase-memory").join("index.db"), b"x").unwrap();

        assert!(!state_may_have_stale_entries(state.path(), DAY * 7));
    }

    #[test]
    fn a_state_folder_with_no_tmp_and_no_index_needs_no_in_use_list() {
        let state = tempfile::tempdir().unwrap();

        assert!(!state_may_have_stale_entries(state.path(), DAY * 7));
    }

    #[test]
    fn an_old_codebase_memory_database_needs_the_in_use_list() {
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir(state.path().join("codebase-memory")).unwrap();
        let db = state.path().join("codebase-memory").join("index.db");
        std::fs::write(&db, b"x").unwrap();
        age(&db);

        assert!(state_may_have_stale_entries(state.path(), DAY * 7));
    }

    #[test]
    fn an_old_tmp_entry_needs_the_in_use_list() {
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir(state.path().join("tmp")).unwrap();
        let old = state.path().join("tmp").join("old");
        std::fs::write(&old, b"x").unwrap();
        age(&old);

        assert!(state_may_have_stale_entries(state.path(), DAY * 7));
    }
}
