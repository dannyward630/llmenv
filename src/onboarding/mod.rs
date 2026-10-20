//! The onboarding flow of `llmenv setup` (#2773): fact primitives that return evidence as JSON
//! for the onboarding skill, and the guarded write that applies its plan. Design:
//! docs/design/issue-2773-onboarding.md (Decision 2).

use std::path::Path;

use anyhow::Context as _;

pub(crate) mod apply;
pub(crate) mod config_splice;
pub(crate) mod plan;
pub(crate) mod scan;
pub(crate) mod tools;

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
