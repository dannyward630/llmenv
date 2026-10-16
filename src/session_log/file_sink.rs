//! Local JSONL sink. Append-only, owner-only, best-effort: a write failure logs
//! at `debug!` and is dropped — session logging never fails a launch.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Context;
use llmenv_paths::{create_dir_owner_only, state_dir};

/// Default file-sink path: `<state_dir>/session-log.jsonl`.
///
/// # Errors
/// Propagates `state_dir()` resolution failure.
pub fn default_file_path() -> anyhow::Result<PathBuf> {
    Ok(state_dir()?.join("session-log.jsonl"))
}

/// Appends rendered events to one JSONL file.
#[derive(Debug, Clone)]
pub struct FileSink {
    path: PathBuf,
}

impl FileSink {
    /// Create a sink writing to `path`. The parent dir is created on first
    /// append.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Append one line (a `\n` is added). Best-effort; errors are logged and
    /// dropped.
    pub(crate) fn append(&self, line: &str) {
        if let Err(e) = self.try_append(line) {
            tracing::debug!("session_log file append failed: {e}");
        }
    }

    fn try_append(&self, line: &str) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            create_dir_owner_only(parent)?;
        }
        let rotated = rotate_if_full(&self.path, MAX_BYTES)?;
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = crate::paths::refuse_symlink(&mut opts).open(&self.path)?;
        // `mode(0o600)` above only applies on creation (O_CREAT); a file that
        // already existed (e.g. created with a looser umask before this sink
        // ran, or by an older llmenv version) keeps its prior permissions. Set
        // them explicitly on every open via the already-open fd so a
        // pre-existing world-readable file gets locked down before this
        // process appends potentially sensitive session-log content to it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        if let Some(name) = rotated {
            f.write_all(rotation_marker(&name).as_bytes())?;
            f.write_all(b"\n")?;
        }
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")
    }
}

/// Size at which the active file rotates. Each line is one hook event, so the cap bounds how
/// much busy hooks add to one file (#2816).
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Rotated generations kept beside the active file. A rotation drops the oldest one.
const KEPT_ROTATIONS: u32 = 3;

/// Rotate `path` when it holds at least `max_bytes`. Returns the name of the first rotation,
/// or `None` when nothing rotated. A rename that loses a race with another process fails, and
/// the caller drops that one event.
fn rotate_if_full(path: &Path, max_bytes: u64) -> std::io::Result<Option<String>> {
    let len = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if len < max_bytes {
        return Ok(None);
    }
    remove_if_present(&rotation_path(path, KEPT_ROTATIONS))?;
    for n in (1..KEPT_ROTATIONS).rev() {
        rename_if_present(&rotation_path(path, n), &rotation_path(path, n + 1))?;
    }
    let first = rotation_path(path, 1);
    std::fs::rename(path, &first)?;
    Ok(first
        .file_name()
        .map(|name| name.to_string_lossy().into_owned()))
}

/// The first line of a file that follows a rotation. It names the file the old lines moved to.
fn rotation_marker(rotated_to: &str) -> String {
    serde_json::json!({"llmenv_log": "rotated", "rotated_to": rotated_to}).to_string()
}

/// The path of rotation `n` of `path`, such as `session-log.jsonl.1`.
fn rotation_path(path: &Path, n: u32) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn rename_if_present(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Remove the rotated files of `path` that were last written more than `older_than` ago. The
/// active file is never removed. Returns how many files were removed (#2816).
///
/// # Errors
/// Propagates a failure to read the directory or to remove a rotated file.
pub fn prune_rotations(path: &Path, older_than: Duration) -> anyhow::Result<usize> {
    let (Some(dir), Some(base)) = (path.parent(), path.file_name()) else {
        return Ok(0);
    };
    let Some(entries) = crate::paths::read_dir_optional(dir)? else {
        return Ok(0);
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        if !is_rotation_of(&entry.file_name(), base) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .with_context(|| format!("stat {}", entry.path().display()))?;
        let age = SystemTime::now().duration_since(modified);
        if age.is_ok_and(|age| age > older_than) {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("removing {}", entry.path().display()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Whether `name` is `<base>.<digits>`, the name of a rotated file.
fn is_rotation_of(name: &std::ffi::OsStr, base: &std::ffi::OsStr) -> bool {
    let (Some(name), Some(base)) = (name.to_str(), base.to_str()) else {
        return false;
    };
    name.strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('.'))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn append_writes_lines_and_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-log.jsonl");
        let sink = FileSink::new(path.clone());
        sink.append("{\"a\":1}");
        sink.append("{\"b\":2}");
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"a\":1}");
        assert_eq!(lines[1], "{\"b\":2}");
    }

    #[cfg(unix)]
    #[test]
    fn append_creates_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        FileSink::new(path.clone()).append("{}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "group/other bits must be unset: {mode:o}");
    }

    // #1186: append creates its parent directory owner-only, not just the file.
    #[cfg(unix)]
    #[test]
    fn append_creates_parent_dir_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("s.jsonl");
        FileSink::new(path.clone()).append("{}");
        let mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "log dir must be owner-only, got {mode:o}");
    }

    #[cfg(unix)]
    #[test]
    fn append_re_protects_a_pre_existing_world_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        FileSink::new(path.clone()).append("{}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "append must lock down a pre-existing looser-permission file: {mode:o}"
        );
    }

    fn write_bytes(path: &Path, len: u64) {
        std::fs::write(path, vec![b'x'; usize::try_from(len).unwrap()]).unwrap();
    }

    #[test]
    fn a_file_under_the_cap_is_not_rotated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-log.jsonl");
        write_bytes(&path, 9);
        assert_eq!(rotate_if_full(&path, 10).unwrap(), None);
        assert!(path.exists());
        assert!(!rotation_path(&path, 1).exists());
    }

    #[test]
    fn a_file_at_the_cap_rotates_to_generation_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-log.jsonl");
        write_bytes(&path, 10);
        let rotated = rotate_if_full(&path, 10).unwrap();
        assert_eq!(rotated.as_deref(), Some("session-log.jsonl.1"));
        assert!(!path.exists(), "the active file must move away");
        assert_eq!(
            std::fs::metadata(rotation_path(&path, 1)).unwrap().len(),
            10
        );
    }

    #[test]
    fn rotations_keep_only_the_last_kept_generations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-log.jsonl");
        let rounds = KEPT_ROTATIONS + 2;
        for round in 0..rounds {
            // Each round writes a distinct size, so the generations can be told apart.
            write_bytes(&path, 10 + u64::from(round));
            rotate_if_full(&path, 10).unwrap();
        }
        for n in 1..=KEPT_ROTATIONS {
            assert!(
                rotation_path(&path, n).exists(),
                "generation {n} is missing"
            );
        }
        assert!(!rotation_path(&path, KEPT_ROTATIONS + 1).exists());
        // Generation 1 holds the newest rotated file, the last round's.
        let last = 10 + u64::from(rounds - 1);
        assert_eq!(
            std::fs::metadata(rotation_path(&path, 1)).unwrap().len(),
            last
        );
    }

    #[test]
    fn a_missing_active_file_is_not_a_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-log.jsonl");
        assert_eq!(rotate_if_full(&path, 10).unwrap(), None);
    }

    #[test]
    fn the_rotation_marker_names_the_file_the_old_lines_moved_to() {
        let marker = rotation_marker("session-log.jsonl.1");
        let value: serde_json::Value = serde_json::from_str(&marker).unwrap();
        assert_eq!(value["llmenv_log"], "rotated");
        assert_eq!(value["rotated_to"], "session-log.jsonl.1");
    }

    #[test]
    fn prune_removes_old_rotations_and_keeps_the_active_file_and_other_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-log.jsonl");
        write_bytes(&path, 3);
        let old_rotation = rotation_path(&path, 2);
        write_bytes(&old_rotation, 3);
        let old_time = SystemTime::now() - Duration::from_secs(30 * 86_400);
        File::options()
            .write(true)
            .open(&old_rotation)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        let unrelated = dir.path().join("session-log.jsonl.bak");
        write_bytes(&unrelated, 3);
        File::options()
            .write(true)
            .open(&unrelated)
            .unwrap()
            .set_modified(old_time)
            .unwrap();

        let removed = prune_rotations(&path, Duration::from_secs(7 * 86_400)).unwrap();

        assert_eq!(removed, 1);
        assert!(!old_rotation.exists());
        assert!(path.exists());
        assert!(
            unrelated.exists(),
            "a name without a numeric suffix is not a rotation"
        );
    }

    #[test]
    fn prune_keeps_a_recent_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-log.jsonl");
        write_bytes(&rotation_path(&path, 1), 3);
        assert_eq!(
            prune_rotations(&path, Duration::from_secs(7 * 86_400)).unwrap(),
            0
        );
        assert!(rotation_path(&path, 1).exists());
    }

    #[test]
    fn an_append_to_a_full_file_starts_the_new_file_with_a_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-log.jsonl");
        // A sparse file of the real cap: no 64 MiB of data is written.
        File::create(&path).unwrap().set_len(MAX_BYTES).unwrap();
        FileSink::new(path.clone()).append("{\"after\":1}");
        let body = std::fs::read_to_string(&path).unwrap();
        let mut lines = body.lines();
        assert_eq!(
            lines.next(),
            Some(rotation_marker("session-log.jsonl.1").as_str())
        );
        assert_eq!(lines.next(), Some("{\"after\":1}"));
        assert!(rotation_path(&path, 1).exists());
    }

    #[test]
    fn prune_in_a_missing_directory_removes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent").join("session-log.jsonl");
        assert_eq!(prune_rotations(&path, Duration::ZERO).unwrap(), 0);
    }
}
