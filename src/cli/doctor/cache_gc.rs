//! The sweeps behind `llmenv doctor --gc`: the hashed cache, then `state/tmp`, then session-log
//! rotations (#2814, #2816).

use std::path::Path;
use std::time::Duration;

use crate::config::Config;
use crate::materialize::{cache, tmp_sweep};
use crate::paths;
use crate::session_log;

use super::forget_credentials_for;

/// Run the sweeps for `doctor --gc`. All three use `cache.cache_retention_hours` (default 168).
pub(super) fn run(cache_dir: &Path, config: &Config, pass: &str, warn: &str) {
    eprintln!("Running garbage collection...");
    // A multiplication that would wrap keeps every entry instead of removing live ones.
    let hours = config.cache.cache_retention_hours.unwrap_or(168);
    let retention = Duration::from_secs(hours.saturating_mul(3600));
    match std::fs::metadata(cache_dir) {
        Ok(meta) if meta.permissions().readonly() => {
            eprintln!("{warn} GC of the cache skipped: cache directory is read-only");
        }
        Ok(_) => sweep_cache(cache_dir, config, retention, pass, warn),
        Err(e) => eprintln!("{warn} GC of the cache skipped: cannot stat cache directory: {e:#}"),
    }
    sweep_state_tmp(retention, pass, warn);
    prune_session_log(retention, pass, warn);
}

/// Sweep the hashed cache under `cache_dir`.
fn sweep_cache(cache_dir: &Path, config: &Config, retention: Duration, pass: &str, warn: &str) {
    match cache::gc(cache_dir, retention, config.cache.hashing) {
        Ok(report) => {
            eprintln!(
                "{pass} GC complete: removed {} entries, kept {}",
                report.removed.len(),
                report.kept
            );
            // #1372: a removal failure no longer aborts the walk,
            // so the entries it skipped have to be reported here
            // or they'd vanish from the summary entirely.
            for p in &report.failed {
                eprintln!("{warn} GC: could not remove {}", p.display());
            }
            let forgotten = forget_credentials_for(&report.removed);
            if forgotten > 0 {
                eprintln!(
                    "{pass} GC: dropped {forgotten} orphaned OAuth credential \
                     entries"
                );
            }
        }
        Err(e) => eprintln!("{warn} GC failed: {e:#}"),
    }
}

/// Sweep `state/tmp`, which `TMPDIR` points at, with the same retention as the cache (#2814).
fn sweep_state_tmp(retention: Duration, pass: &str, warn: &str) {
    let swept = paths::state_dir().and_then(|dir| tmp_sweep::sweep_state_tmp(&dir, retention));
    match swept {
        Ok(report) => {
            eprintln!(
                "{pass} GC complete for state/tmp: removed {} entries, kept {}",
                report.removed.len(),
                report.kept
            );
            for p in &report.failed {
                eprintln!("{warn} GC: could not remove {}", p.display());
            }
        }
        Err(e) => eprintln!("{warn} GC of state/tmp skipped: {e:#}"),
    }
}

/// Remove session-log rotations older than the retention (#2816). The active file is kept.
fn prune_session_log(retention: Duration, pass: &str, warn: &str) {
    let pruned = session_log::default_file_path()
        .and_then(|path| session_log::rotation::prune_rotations(&path, retention));
    match pruned {
        Ok(count) => eprintln!("{pass} GC: removed {count} rotated session-log file(s)"),
        Err(e) => eprintln!("{warn} GC of session-log rotations failed: {e:#}"),
    }
}
