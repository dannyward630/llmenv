//! Retention for the index databases under `state/codebase-memory` (#2817).
//!
//! llmenv passes `CBM_CACHE_DIR` to codebase-memory-mcp only when `index_path` is set. Without
//! it, the server keeps its indexes in its own default cache, and llmenv keeps only its index
//! log and index results in `state/codebase-memory`. A database file there is left from an
//! older setup that no server reads, so an old one can go. A folder that a configured
//! `index_path` names is the live cache, and it is never swept.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::materialize::cache::{GcReport, gc_remove_link, is_expired};

/// The file name endings of a codebase-memory-mcp index database and its SQLite side files.
const DB_SUFFIXES: [&str; 3] = [".db", ".db-wal", ".db-shm"];

/// Remove the index database files in `dir` that were last written more than `older_than`
/// ago. Other files, such as the index log and the index results, are kept. A file that cannot
/// be judged or removed is recorded in the report, and the sweep goes on.
///
/// # Errors
/// `dir` exists but cannot be read.
pub(crate) fn sweep_legacy_indexes(dir: &Path, older_than: Duration) -> anyhow::Result<GcReport> {
    let mut report = GcReport::default();
    let Some(entries) = crate::paths::read_dir_optional(dir)? else {
        return Ok(report);
    };
    let now = SystemTime::now();
    let readable =
        entries.filter_map(|entry| crate::paths::readable_entry(dir, "cbm sweep", entry));
    for entry in readable {
        let path = entry.path();
        let is_db = entry
            .file_name()
            .to_str()
            .is_some_and(|name| DB_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)));
        if !is_db {
            continue;
        }
        // `symlink_metadata`: a link is judged and removed as a link, never followed.
        match std::fs::symlink_metadata(&path).and_then(|meta| meta.modified()) {
            Ok(modified) if is_expired(now, modified, older_than) => {
                gc_remove_link(&path, &mut report);
            }
            Ok(_) => report.kept += 1,
            Err(e) => {
                tracing::error!("cbm sweep cannot read the age of {}: {e}", path.display());
                report.failed.push(path);
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn backdate(path: &Path) {
        let old = SystemTime::now() - Duration::from_secs(30 * 86_400);
        filetime::set_file_mtime(path, filetime::FileTime::from_system_time(old)).unwrap();
    }

    #[test]
    fn only_old_database_files_go() {
        let dir = tempfile::tempdir().unwrap();
        let old_db = dir.path().join("Users-x-repo.db");
        let old_wal = dir.path().join("Users-x-repo.db-wal");
        let new_db = dir.path().join("Users-x-live.db");
        let log = dir.path().join("index.log");
        let result = dir.path().join("index-result-1.json");
        for p in [&old_db, &old_wal, &new_db, &log, &result] {
            std::fs::write(p, "x").unwrap();
        }
        for p in [&old_db, &old_wal, &log, &result] {
            backdate(p);
        }

        let report = sweep_legacy_indexes(dir.path(), Duration::from_secs(7 * 86_400)).unwrap();

        assert_eq!(report.removed.len(), 2, "{report:?}");
        assert!(!old_db.exists() && !old_wal.exists());
        assert!(new_db.exists() && log.exists() && result.exists());
        assert_eq!(report.kept, 1);
        assert!(report.failed.is_empty());
    }

    #[test]
    fn a_missing_folder_is_an_empty_report() {
        let dir = tempfile::tempdir().unwrap();
        let report = sweep_legacy_indexes(&dir.path().join("absent"), Duration::ZERO).unwrap();
        assert!(report.removed.is_empty() && report.failed.is_empty());
    }
}
