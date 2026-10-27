//! Process-wide count of files the shared owner-only writers have written.
//!
//! The writers also skip a file that already holds the same bytes at the same mode. A skip is
//! counted apart from a write, so a repeat run shows `files == 0` (#3020).
//!
//! A caller that wants "how many files did this step write" takes a [`snapshot`] before the step
//! and calls [`WriteStats::since`] after it (#3020).

use std::sync::atomic::{AtomicU64, Ordering};

static FILES: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);

/// A count of files written and the bytes they held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriteStats {
    /// Files written.
    pub files: u64,
    /// Bytes written.
    pub bytes: u64,
    /// Files left alone because they already held the same bytes at the same mode.
    pub skipped: u64,
}

impl WriteStats {
    /// The writes made after `earlier` was taken.
    #[must_use]
    pub fn since(self, earlier: WriteStats) -> WriteStats {
        WriteStats {
            files: self.files.saturating_sub(earlier.files),
            bytes: self.bytes.saturating_sub(earlier.bytes),
            skipped: self.skipped.saturating_sub(earlier.skipped),
        }
    }
}

/// Record one finished write of `bytes` bytes.
pub(crate) fn note_write(bytes: usize) {
    FILES.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// Whether `path` is a regular file that holds `content` at permission bits `mode`.
///
/// A symlink, a missing file, a different size, a different mode, or an unreadable file all give
/// `false`, and the caller then writes as usual. A `true` is recorded as a skip.
pub(crate) fn already_current(path: &std::path::Path, content: &[u8], mode: u32) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return false;
        };
        let same_shape = meta.is_file()
            && meta.permissions().mode() & 0o777 == mode
            && usize::try_from(meta.len()).is_ok_and(|len| len == content.len());
        if same_shape && std::fs::read(path).is_ok_and(|old| old == content) {
            SKIPPED.fetch_add(1, Ordering::Relaxed);
            return true;
        }
        false
    }
    #[cfg(not(unix))]
    {
        let _ = (path, content, mode);
        false
    }
}

/// The writes made by this process so far.
#[must_use]
pub fn snapshot() -> WriteStats {
    WriteStats {
        files: FILES.load(Ordering::Relaxed),
        bytes: BYTES.load(Ordering::Relaxed),
        skipped: SKIPPED.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn since_counts_only_the_writes_after_the_snapshot() {
        let before = snapshot();
        note_write(5);
        note_write(7);
        let delta = snapshot().since(before);
        assert!(delta.files >= 2 && delta.bytes >= 12);
    }

    #[cfg(unix)]
    #[test]
    fn already_current_needs_same_bytes_and_same_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        assert!(
            !already_current(&path, b"abc", 0o600),
            "a missing file is not current"
        );
        std::fs::write(&path, b"abc").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(already_current(&path, b"abc", 0o600));
        assert!(!already_current(&path, b"abd", 0o600), "other bytes");
        assert!(!already_current(&path, b"abcd", 0o600), "other size");
        assert!(!already_current(&path, b"abc", 0o700), "other mode");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            !already_current(&path, b"abc", 0o600),
            "a loose file is rewritten"
        );
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(
            !already_current(&link, b"abc", 0o644),
            "a symlink is never current"
        );
    }
}
