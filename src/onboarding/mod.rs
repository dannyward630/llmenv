//! The onboarding flow of `llmenv setup` (#2773): fact primitives that return evidence as JSON
//! for the onboarding skill, and the guarded write that applies its plan. Design:
//! docs/design/issue-2773-onboarding.md (Decision 2).

use std::path::{Path, PathBuf};

use anyhow::Context as _;

pub(crate) mod apply;
pub(crate) mod config_splice;
pub(crate) mod plan;
pub(crate) mod scan;
pub(crate) mod shipped;
pub(crate) mod tools;

/// A file that `llmenv setup --apply` creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewFile {
    /// The path relative to the config folder.
    pub(crate) rel: PathBuf,
    pub(crate) text: String,
    /// Create the file with the executable bit, because a skill runs it directly.
    pub(crate) executable: bool,
}

/// Whether anything, a link included, is at `path`.
///
/// # Errors
/// The check fails for a reason other than that nothing is there, such as a folder that cannot
/// be read. Such a path is neither safe to skip nor safe to create.
pub(crate) fn path_exists(path: &Path) -> anyhow::Result<bool> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("checking {}", path.display())),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::path_exists;

    #[test]
    fn a_missing_path_is_absent_and_any_other_error_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(path_exists(&file).unwrap());
        assert!(!path_exists(&dir.path().join("missing")).unwrap());
        // A path below a regular file fails with "not a directory", which is not "absent".
        let err = path_exists(&file.join("below")).unwrap_err();
        assert!(format!("{err:#}").contains("checking "), "{err:#}");
    }
}
