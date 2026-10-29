//! The newest mtime under a cache folder, for the cache GC age check.

use std::path::Path;
use std::time::SystemTime;

use anyhow::Context as _;

use super::skip_vanished;

/// Newest mtime found anywhere under `dir` (including the dir itself).
pub(crate) fn newest_mtime(dir: &Path) -> anyhow::Result<SystemTime> {
    let mut newest = dir
        .metadata()
        .and_then(|m| m.modified())
        .with_context(|| format!("stat {}", dir.display()))?;
    walk_mtime(dir, &mut newest)?;
    Ok(newest)
}

/// A child that another tool removes during the walk is skipped, not an error (#2835).
fn walk_mtime(dir: &Path, newest: &mut SystemTime) -> anyhow::Result<()> {
    let Some(entries) = skip_vanished(std::fs::read_dir(dir))
        .with_context(|| format!("reading {}", dir.display()))?
    else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry under {}", dir.display()))?;
        let p = entry.path();
        // `DirEntry::metadata` does not follow a symlink, so one stat gives both the type and
        // the mtime. A symlink counts by its own mtime and is never descended, the same as the
        // other GC sweeps (#3076).
        let Some(meta) =
            skip_vanished(entry.metadata()).with_context(|| format!("stat {}", p.display()))?
        else {
            continue;
        };
        let m = meta
            .modified()
            .with_context(|| format!("mtime {}", p.display()))?;
        if m > *newest {
            *newest = m;
        }
        if meta.is_dir() {
            walk_mtime(&p, newest)?;
        }
    }
    Ok(())
}
