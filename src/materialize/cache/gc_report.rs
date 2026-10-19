//! The report of an age-based sweep, and the two removal helpers that fill it (#1372, #2917).

use std::path::{Path, PathBuf};

use super::remove_dir_all_forcing_writable;

/// No `Default`: each sweep must choose `dry_run` through [`GcReport::new`], so a new sweep cannot
/// delete by accident when its caller asked for a preview.
#[derive(Debug)]
pub struct GcReport {
    pub removed: Vec<PathBuf>,
    pub kept: usize,
    /// Entries gc tried to remove but couldn't. Same contract as
    /// [`super::PruneReport::failed`] — never counted as `removed` (#1372).
    pub(crate) failed: Vec<PathBuf>,
    /// When set, the sweep records each removal in `removed` and deletes nothing (#2917).
    pub(crate) dry_run: bool,
}

impl GcReport {
    /// An empty report for a sweep that deletes, or only previews when `dry_run` is set.
    pub(crate) fn new(dry_run: bool) -> Self {
        Self {
            removed: Vec::new(),
            kept: 0,
            failed: Vec::new(),
            dry_run,
        }
    }
}

/// Remove a directory for [`gc`](super::gc), recording rather than propagating a failure.
///
/// Same reasoning as [`remove_dir`](super::remove_dir) (#1372): one undeletable entry must not
/// abort collection of everything after it, since `doctor --gc` reported only
/// the bare errno and left the rest of the cache untouched.
pub(crate) fn gc_remove_dir(p: &Path, report: &mut GcReport) {
    gc_remove(
        p,
        report,
        "remove cache directory",
        remove_dir_all_forcing_writable,
    );
}

/// Unlink a symlink or a file for [`gc`](super::gc), recording rather than propagating a failure.
pub(crate) fn gc_remove_link(p: &Path, report: &mut GcReport) {
    gc_remove(p, report, "unlink cache entry", |p| std::fs::remove_file(p));
}

/// Record `p` as removed. A dry run stops there; a real run calls `remove` first, and records a
/// failure instead when it fails.
fn gc_remove(
    p: &Path,
    report: &mut GcReport,
    action: &str,
    remove: impl FnOnce(&Path) -> std::io::Result<()>,
) {
    if report.dry_run {
        report.removed.push(p.to_path_buf());
        return;
    }
    match remove(p) {
        Ok(()) => report.removed.push(p.to_path_buf()),
        Err(e) => {
            tracing::error!(
                path = %p.display(),
                error = %e,
                "gc: failed to {action}; skipping"
            );
            report.failed.push(p.to_path_buf());
        }
    }
}
