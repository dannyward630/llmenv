//! Moves the `projects/` trees that older layouts left in hashed folders into the durable store.

use std::path::{Path, PathBuf};

use anyhow::Context as _;

use super::PROJECTS_DIR;
use super::fold::move_dir_newest_wins;

/// Move every `projects/` tree stranded in an old hashed folder into the durable
/// store, newest mtime winning per file. Returns how many trees were folded.
///
/// Handles both layouts: `<root>/<shape>/projects` (loose, strict) and
/// `<root>/<version>/<shape>/projects` (normal) — the old code only ever scanned
/// the flat one, which is why transcripts piled up unmigrated. The now-empty
/// source directories are left for `llmenv clean` to reclaim.
///
/// # Errors
/// Returns an error when a tree cannot be folded into the durable store.
pub(crate) fn migrate_stranded_projects(
    adapter_root: &Path,
    state_dir: &Path,
) -> anyhow::Result<usize> {
    let target = state_dir.join(PROJECTS_DIR);
    let mut folded = 0usize;
    for src in stranded_projects_dirs(adapter_root, state_dir) {
        let skipped = move_dir_newest_wins(&src, &target)
            .with_context(|| format!("folding stranded transcripts from {}", src.display()))?;
        if !skipped.is_empty() {
            tracing::warn!(
                "{} entries stay in {}, which the fold does not move",
                skipped.len(),
                src.display()
            );
        }
        folded = folded.saturating_add(1);
    }
    Ok(folded)
}

/// Every stranded `projects/` directory under `adapter_root`, at both the flat
/// and version-nested depths. Excludes the durable store and already-linked
/// folders (following a link would fold the store into itself).
fn stranded_projects_dirs(adapter_root: &Path, state_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for level1 in child_dirs(adapter_root) {
        if level1 == state_dir {
            continue;
        }
        push_projects_dir(&level1, &mut out);
        for level2 in child_dirs(&level1) {
            push_projects_dir(&level2, &mut out);
        }
    }
    out
}

/// Immediate subdirectories of `dir`, skipping symlinks and staging dirs.
/// An unreadable directory yields nothing — migration is best-effort.
fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    // An absent dir is nothing to migrate; anything else (a permission error,
    // say) is warned rather than swallowed — treating it as "no children" would
    // silently skip a folder full of transcripts.
    let read_dir = match crate::paths::read_dir_optional(dir) {
        Ok(Some(rd)) => rd,
        Ok(None) => return Vec::new(),
        Err(e) => {
            tracing::error!(
                "inherit: cannot scan {} for transcripts: {e:#}",
                dir.display()
            );
            return Vec::new();
        }
    };
    // Each skipped entry is logged at error level, because its transcripts are not migrated.
    read_dir
        .filter_map(|entry| crate::paths::readable_entry(dir, "transcript migration", entry))
        .filter(|entry| !entry.file_name().as_encoded_bytes().ends_with(b".tmp"))
        .filter_map(|entry| match entry.file_type() {
            Ok(ft) => ft.is_dir().then(|| entry.path()),
            Err(e) => {
                tracing::error!(
                    "inherit: cannot read the type of {}, so its transcripts are not migrated: {e}",
                    entry.path().display()
                );
                None
            }
        })
        .collect()
}

/// Push `<dir>/projects` onto `out` when it is a real directory (not a link).
fn push_projects_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let candidate = dir.join(PROJECTS_DIR);
    if std::fs::symlink_metadata(&candidate).is_ok_and(|md| md.file_type().is_dir()) {
        out.push(candidate);
    }
}
