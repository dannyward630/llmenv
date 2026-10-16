//! The sweeps behind `llmenv doctor --gc`: the hashed cache, then `state/tmp` (#2814).

use std::path::Path;
use std::time::Duration;

use crate::config::Config;
use crate::materialize::{cache, tmp_sweep};
use crate::paths;
use crate::session_log;

use super::forget_credentials_for;

/// Run the sweeps for `doctor --gc`. Both use `cache.cache_retention_hours` (default 168).
pub(super) fn run(cache_dir: &Path, config: &Config, pass: &str, warn: &str) {
    eprintln!("Running garbage collection...");
    match std::fs::metadata(cache_dir) {
        Ok(meta) if meta.permissions().readonly() => {
            eprintln!("{warn} GC failed: cache directory is read-only");
        }
        Ok(_) => sweep_cache(cache_dir, config, pass, warn),
        Err(e) => eprintln!("{warn} GC failed to stat cache directory: {}", e),
    }
}

/// Sweep the hashed cache under `cache_dir`, then the state temp folder.
fn sweep_cache(cache_dir: &Path, config: &Config, pass: &str, warn: &str) {
    let retention = Duration::from_secs(config.cache.cache_retention_hours.unwrap_or(168) * 3600);
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
        Err(e) => eprintln!("{warn} GC failed: {}", e),
    }
    sweep_state_tmp(retention, pass, warn);
    prune_session_log(retention, pass, warn);
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
        .and_then(|path| session_log::file_sink::prune_rotations(&path, retention));
    match pruned {
        Ok(count) => eprintln!("{pass} GC: removed {count} rotated session-log file(s)"),
        Err(e) => eprintln!("{warn} GC of session-log rotations failed: {e:#}"),
    }
}
