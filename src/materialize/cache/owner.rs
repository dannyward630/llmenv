//! Finds a path that another user owns inside a tree that a sweep could not remove (#3184).
//!
//! A sweep runs as the user, so it cannot remove a file that root or another user owns in a
//! folder it cannot change. Such a file stops the removal part-way, and the error alone does not
//! say which file, or what to do.

use std::path::{Path, PathBuf};

/// The cause text for a failed removal of the tree at `p`. When a path in the tree has another
/// owner, the text names that path, its owner, and the fix.
pub(super) fn removal_cause(p: &Path, e: &std::io::Error) -> String {
    #[cfg(unix)]
    {
        let me = rustix::process::geteuid().as_raw();
        if let Some((path, owner)) = foreign_owned(p, me) {
            return format!(
                "{e}. {} is owned by uid {owner}, not by this user (uid {me}), so the sweep \
                 cannot remove it. Fix: remove {} as uid {owner}, for example with sudo, then \
                 run `llmenv clean` again",
                path.display(),
                p.display()
            );
        }
    }
    e.to_string()
}

/// The first path under `p`, `p` included, that a user other than `me` owns. A symlink is judged
/// by its own owner and is never followed. An entry that cannot be read is skipped.
#[cfg(unix)]
fn foreign_owned(p: &Path, me: u32) -> Option<(PathBuf, u32)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(p).ok()?;
    if meta.uid() != me {
        return Some((p.to_path_buf(), meta.uid()));
    }
    if !meta.is_dir() {
        return None;
    }
    std::fs::read_dir(p)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| foreign_owned(&entry.path(), me))
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_that_the_user_owns_has_no_foreign_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("f"), b"x").unwrap();
        let me = rustix::process::geteuid().as_raw();
        assert_eq!(foreign_owned(dir.path(), me), None);
    }

    #[test]
    fn a_path_of_another_owner_is_named_with_its_uid() {
        let dir = tempfile::tempdir().unwrap();
        let me = rustix::process::geteuid().as_raw();
        let other = me.wrapping_add(1);
        assert_eq!(
            foreign_owned(dir.path(), other),
            Some((dir.path().to_path_buf(), me))
        );
    }

    #[test]
    fn a_cause_without_a_foreign_owner_is_the_error_alone() {
        let dir = tempfile::tempdir().unwrap();
        let e = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(removal_cause(dir.path(), &e), e.to_string());
    }
}
