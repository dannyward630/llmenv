//! Groups the index databases under `state/codebase-memory` into units and reads each unit's age
//! (#2817). The sweep removes the expired units, and the state GC gate asks whether any unit could
//! be removed. Both use this one scan, so they agree on what is removable (#3072).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Context as _;

use crate::materialize::cache::is_expired;

/// The file name endings of a codebase-memory-mcp index database and its SQLite side files.
const DB_SUFFIXES: [&[u8]; 3] = [b".db", b".db-wal", b".db-shm"];

/// The name bytes of the database a file belongs to, or `None` for a file that is not an index
/// database. The name is compared as bytes, so a non-UTF-8 name is still judged.
fn database_of(name: &std::ffi::OsStr) -> Option<Vec<u8>> {
    let bytes = name.as_encoded_bytes();
    DB_SUFFIXES
        .iter()
        .find_map(|suffix| bytes.strip_suffix(*suffix))
        .map(<[u8]>::to_vec)
}

/// How old one unit is, compared with the retention.
#[derive(Debug)]
enum Age {
    /// The newest file is past the retention, so the unit may be removed.
    Expired,
    /// The newest file is within the retention, so the unit is kept.
    Fresh,
    /// The age of this file cannot be read. The unit is kept, and it is reported as failed.
    Unreadable(PathBuf, std::io::Error),
}

/// One index database and its side files. The unit is judged as a whole.
#[derive(Debug)]
pub(crate) struct Unit {
    pub(crate) files: Vec<PathBuf>,
    age: Age,
}

impl Unit {
    /// Whether the newest file is past the retention.
    pub(crate) fn is_expired(&self) -> bool {
        matches!(self.age, Age::Expired)
    }

    /// Whether the newest file is within the retention.
    pub(crate) fn is_fresh(&self) -> bool {
        matches!(self.age, Age::Fresh)
    }

    /// The file whose age cannot be read, and its error, or `None` when every age was read.
    pub(crate) fn unreadable(&self) -> Option<(&Path, &std::io::Error)> {
        match &self.age {
            Age::Unreadable(file, e) => Some((file.as_path(), e)),
            Age::Expired | Age::Fresh => None,
        }
    }
}

/// Group the index databases in `dir` into units, and read the age of each unit once. `None`
/// means `dir` does not exist.
///
/// # Errors
/// `dir` exists but cannot be read, or one of its entries cannot be read.
pub(crate) fn scan_units(dir: &Path, older_than: Duration) -> anyhow::Result<Option<Vec<Unit>>> {
    let Some(entries) = crate::paths::read_dir_optional(dir)? else {
        return Ok(None);
    };
    let mut databases: BTreeMap<Vec<u8>, Vec<PathBuf>> = BTreeMap::new();
    for entry in entries {
        // An unread entry can be the `-wal` of a database that is still in use, so no unit can be
        // judged complete. The scan stops, and the caller removes nothing.
        let entry = entry.with_context(|| {
            format!(
                "cannot read an entry of {}, so no index database was removed. Fix: check the \
                 permissions of the folder",
                dir.display()
            )
        })?;
        if let Some(base) = database_of(&entry.file_name()) {
            databases.entry(base).or_default().push(entry.path());
        }
    }
    let now = SystemTime::now();
    let units = databases
        .into_values()
        .map(|files| {
            let age = match newest_of(&files) {
                Ok(newest) if is_expired(now, newest, older_than, dir) => Age::Expired,
                Ok(_) => Age::Fresh,
                Err((file, e)) => Age::Unreadable(file, e),
            };
            Unit { files, age }
        })
        .collect();
    Ok(Some(units))
}

/// The newest mtime of `files`, read without following a symlink.
fn newest_of(files: &[PathBuf]) -> Result<SystemTime, (PathBuf, std::io::Error)> {
    let mut newest = SystemTime::UNIX_EPOCH;
    for file in files {
        let modified = std::fs::symlink_metadata(file)
            .and_then(|meta| meta.modified())
            .map_err(|e| (file.clone(), e))?;
        newest = newest.max(modified);
    }
    Ok(newest)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::database_of;

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_database_name_is_judged() {
        use std::os::unix::ffi::OsStrExt as _;
        let name = std::ffi::OsStr::from_bytes(b"repo-\xff.db-shm");
        let base = database_of(name).unwrap();
        assert_eq!(base, b"repo-\xff");
        assert!(database_of(std::ffi::OsStr::new("index.log")).is_none());
    }
}
