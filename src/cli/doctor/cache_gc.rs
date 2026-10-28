//! The sweeps behind `llmenv clean` and the deprecated `llmenv doctor --gc`: the hashed cache,
//! then `state/tmp`, then session-log rotations, then old codebase-memory indexes (#2814, #2816,
//! #2917).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::Config;
use crate::materialize::cache::GcReport;
use crate::materialize::{cache, tmp_sweep};
use crate::paths;

/// How the sweeps print their result, and whether they remove anything.
#[derive(Debug, Clone, Copy)]
pub(in crate::cli) struct GcOutput<'a> {
    pub(in crate::cli) pass: &'a str,
    pub(in crate::cli) warn: &'a str,
    /// List what a real run removes, and remove nothing (#2917).
    pub(in crate::cli) dry_run: bool,
}

impl GcOutput<'_> {
    fn verb(&self) -> &'static str {
        if self.dry_run {
            "would remove"
        } else {
            "removed"
        }
    }

    /// Print each entry of a dry run, so the preview names every item (#2917).
    fn list(&self, removed: &[PathBuf]) {
        if self.dry_run {
            for p in removed {
                eprintln!("  would remove: {}", p.display());
            }
        }
    }

    /// Print the summary line, the dry-run list, and each failure of one sweep.
    fn report(&self, what: &str, report: &GcReport) {
        let (pass, warn) = (self.pass, self.warn);
        eprintln!(
            "{pass} GC complete{what}: {} {} entries, kept {}",
            self.verb(),
            report.removed.len(),
            report.kept
        );
        self.list(&report.removed);
        // #1372: a removal failure does not abort the walk, so the skipped entries are named here.
        for p in &report.failed {
            eprintln!("{warn} GC: could not process {}", p.display());
        }
    }
}

mod credentials;
mod index_use;
mod session_rotation;
mod state_gate;
pub(in crate::cli) use credentials::forget_credentials_for;
use index_use::{CacheUse, bundles_dir, cache_use};
use session_rotation::prune_session_log;
use state_gate::state_may_have_stale_entries;

/// Run every sweep, and return how many entries or sweeps failed. All of them use
/// `cache.cache_retention_hours` (default 168). A `null` value turns all of them off (#2829).
///
/// A failure is printed as a warning here. `llmenv clean` turns a nonzero count into an error, so
/// a script does not read a skipped sweep as a finished one.
pub(in crate::cli) fn run(cache_dir: &Path, config: &Config, out: GcOutput<'_>) -> usize {
    eprintln!("Running garbage collection...");
    let Some(retention) = config.cache.gc_retention() else {
        eprintln!(
            "{} GC skipped: cache.cache_retention_hours is null, so age-based GC is off. \
             Set it to a number of hours to turn GC on.",
            out.pass
        );
        return 0;
    };
    let mut failures = match std::fs::metadata(cache_dir) {
        Ok(meta) if meta.permissions().readonly() => {
            eprintln!(
                "{} GC of the cache skipped: cache directory is read-only",
                out.warn
            );
            1
        }
        Ok(_) => sweep_cache(cache_dir, config, retention, out),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => {
            eprintln!(
                "{} GC of the cache skipped: cannot stat cache directory: {e:#}",
                out.warn
            );
            1
        }
    };
    match paths::state_dir() {
        Ok(state_dir) => failures += sweep_state(&state_dir, config, retention, out),
        Err(e) => {
            eprintln!(
                "{} GC of state/tmp, session-log rotations, and codebase-memory indexes skipped: \
                 cannot resolve the state directory: {e:#}",
                out.warn
            );
            failures += 1;
        }
    }
    failures
}

/// Sweep the hashed cache under `cache_dir`. Returns the count of failed entries.
fn sweep_cache(cache_dir: &Path, config: &Config, retention: Duration, out: GcOutput<'_>) -> usize {
    match cache::gc(cache_dir, retention, config.cache.hashing, out.dry_run) {
        Ok(report) => {
            out.report("", &report);
            if !out.dry_run {
                let forgotten = forget_credentials_for(&report.removed);
                if forgotten > 0 {
                    eprintln!(
                        "{} GC: dropped {forgotten} orphaned OAuth credential entries",
                        out.pass
                    );
                }
            }
            report.failed.len()
        }
        Err(e) => {
            eprintln!("{} GC failed: {e:#}", out.warn);
            1
        }
    }
}

/// Sweep the folders under `state_dir`: `state/tmp`, the session-log rotations, and the old
/// codebase-memory indexes. Returns the count of failures.
///
/// The in-use list comes from one lsof call, which the two sweeps share. lsof can take up to a
/// minute on a hung mount, so the call runs only when a sweep may remove something (#3039).
fn sweep_state(state_dir: &Path, config: &Config, retention: Duration, out: GcOutput<'_>) -> usize {
    // With nothing old enough to remove, the tmp and index sweeps are skipped. A sweep that ran
    // now could see an entry that crossed the cutoff after this check, and remove it without
    // the in-use list.
    if !state_may_have_stale_entries(state_dir, retention) {
        return prune_session_log(state_dir, retention, out);
    }
    let in_use = crate::materialize::open_files::open_paths().map_err(|e| format!("{e:#}"));
    let in_use = in_use.as_ref().map(Vec::as_slice).map_err(String::as_str);
    sweep_state_tmp(state_dir, retention, in_use, out)
        + prune_session_log(state_dir, retention, out)
        + sweep_cbm_indexes(state_dir, config, retention, in_use, out)
}

/// Sweep `state/tmp`, which `TMPDIR` points at, with the same retention as the cache (#2814).
fn sweep_state_tmp(
    state_dir: &Path,
    retention: Duration,
    in_use: Result<&[PathBuf], &str>,
    out: GcOutput<'_>,
) -> usize {
    let swept = in_use.map_err(str::to_string).and_then(|in_use| {
        tmp_sweep::sweep(&state_dir.join("tmp"), retention, in_use, out.dry_run)
            .map_err(|e| format!("{e:#}"))
    });
    match swept {
        Ok(report) => {
            out.report(" for state/tmp", &report);
            report.failed.len()
        }
        Err(e) => {
            eprintln!("{} GC of state/tmp skipped: {e}", out.warn);
            1
        }
    }
}

/// Remove old index databases from `state/codebase-memory`, unless a configured `index_path`
/// names that folder as the live cache (#2817). A database that a process holds open is kept.
fn sweep_cbm_indexes(
    state_dir: &Path,
    config: &Config,
    retention: Duration,
    in_use: Result<&[PathBuf], &str>,
    out: GcOutput<'_>,
) -> usize {
    let (pass, warn) = (out.pass, out.warn);
    let dir = state_dir.join("codebase-memory");
    // lsof reports resolved paths, so the folder is resolved too before the paths are compared.
    let resolved = match std::fs::canonicalize(&dir) {
        Ok(resolved) => resolved,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(e) => {
            eprintln!(
                "{warn} GC of {} skipped: cannot resolve the folder: {e}",
                dir.display()
            );
            return 1;
        }
    };
    let used = match bundles_dir() {
        Ok(bundles) => cache_use(config, &bundles, &dir),
        Err(e) if !config.bundle.is_empty() => CacheUse::Unknown(format!(
            "cannot resolve the config folder, so the index_path of each bundle is unknown: {e:#}"
        )),
        Err(_) => cache_use(config, Path::new(""), &dir),
    };
    match used {
        CacheUse::Unused => {}
        CacheUse::Live => {
            eprintln!(
                "{pass} GC of {} skipped: a features.codebase_memory index_path uses it",
                dir.display()
            );
            return 0;
        }
        CacheUse::Unknown(cause) => {
            eprintln!("{warn} GC of {} skipped: {cause}", dir.display());
            return 1;
        }
    }
    let swept = in_use.map_err(str::to_string).and_then(|in_use| {
        crate::materialize::cbm_sweep::sweep_legacy_indexes(
            &resolved,
            retention,
            in_use,
            out.dry_run,
        )
        .map_err(|e| format!("{e:#}"))
    });
    match swept {
        Ok(report) => {
            out.report(" for old codebase-memory indexes", &report);
            report.failed.len()
        }
        Err(e) => {
            eprintln!("{warn} GC of old codebase-memory indexes skipped: {e}");
            1
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs::File;
    use std::time::{Duration, SystemTime};

    use super::{GcOutput, sweep_cache, sweep_state};
    use crate::config::Config;

    const DAY: Duration = Duration::from_secs(86_400);
    const OUT: GcOutput<'static> = GcOutput {
        pass: "ok",
        warn: "warn",
        dry_run: false,
    };

    #[test]
    fn a_configured_index_path_marks_the_folder_live() {
        let state = tempfile::tempdir().unwrap();
        let dir = state.path().join("codebase-memory");
        std::fs::create_dir(&dir).unwrap();
        let with = |index_path: &str| -> Config {
            serde_yaml::from_str(&format!(
                "features:\n  codebase_memory:\n    - index_path: {index_path}\n"
            ))
            .unwrap()
        };
        use super::{CacheUse, cache_use};
        assert_eq!(
            cache_use(&with(&dir.display().to_string()), state.path(), &dir),
            CacheUse::Live
        );
        assert_eq!(
            cache_use(&with("/elsewhere/cbm"), state.path(), &dir),
            CacheUse::Unused
        );
        assert_eq!(
            cache_use(&Config::default(), state.path(), &dir),
            CacheUse::Unused
        );
    }

    #[test]
    fn a_bundle_index_path_marks_the_folder_live() {
        use super::{CacheUse, cache_use};
        let state = tempfile::tempdir().unwrap();
        let dir = state.path().join("codebase-memory");
        std::fs::create_dir(&dir).unwrap();
        let bundles = state.path().join("bundles");
        std::fs::create_dir_all(bundles.join("work")).unwrap();
        std::fs::write(
            bundles.join("work/bundle.yaml"),
            format!(
                "features:\n  codebase_memory:\n    - when: [work]\n      index_path: {}\n",
                dir.display()
            ),
        )
        .unwrap();
        let config: Config = serde_yaml::from_str("bundle:\n  - name: work\n").unwrap();

        assert_eq!(cache_use(&config, &bundles, &dir), CacheUse::Live);
    }

    #[test]
    fn an_unreadable_bundle_leaves_the_folder_use_unknown() {
        use super::{CacheUse, cache_use};
        let state = tempfile::tempdir().unwrap();
        let dir = state.path().join("codebase-memory");
        std::fs::create_dir(&dir).unwrap();
        let bundles = state.path().join("bundles");
        std::fs::create_dir_all(bundles.join("work")).unwrap();
        std::fs::write(
            bundles.join("work/bundle.yaml"),
            "features: [not, a, mapping]\n",
        )
        .unwrap();
        let config: Config = serde_yaml::from_str("bundle:\n  - name: work\n").unwrap();

        assert!(matches!(
            cache_use(&config, &bundles, &dir),
            CacheUse::Unknown(_)
        ));
    }

    #[test]
    fn the_state_sweep_removes_an_old_rotation_and_keeps_the_active_log() {
        let state = tempfile::tempdir().unwrap();
        let active = state.path().join("session-log.jsonl");
        std::fs::write(&active, b"{}").unwrap();
        let rotated = state.path().join("session-log.jsonl.1");
        std::fs::write(&rotated, b"{}").unwrap();
        File::options()
            .write(true)
            .open(&rotated)
            .unwrap()
            .set_modified(SystemTime::now() - 30 * DAY)
            .unwrap();

        sweep_state(state.path(), &Config::default(), DAY * 7, OUT);

        assert!(!rotated.exists(), "the old rotation was kept");
        assert!(active.exists(), "the active log was removed");
    }

    #[test]
    fn a_dry_run_of_the_state_sweep_keeps_an_old_rotation() {
        let state = tempfile::tempdir().unwrap();
        let rotated = state.path().join("session-log.jsonl.1");
        std::fs::write(&rotated, b"{}").unwrap();
        File::options()
            .write(true)
            .open(&rotated)
            .unwrap()
            .set_modified(SystemTime::now() - 30 * DAY)
            .unwrap();

        let preview = GcOutput {
            dry_run: true,
            ..OUT
        };
        sweep_state(state.path(), &Config::default(), DAY * 7, preview);

        assert!(rotated.exists(), "a dry run removed an old rotation");
    }

    #[test]
    fn the_state_sweep_with_no_tmp_folder_and_no_log_changes_nothing() {
        let state = tempfile::tempdir().unwrap();
        sweep_state(state.path(), &Config::default(), DAY * 7, OUT);
        assert_eq!(std::fs::read_dir(state.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_cache_sweep_keeps_a_fresh_entry() {
        let cache = tempfile::tempdir().unwrap();
        let entry = cache.path().join("fresh");
        std::fs::create_dir(&entry).unwrap();
        let failures = sweep_cache(cache.path(), &Config::default(), DAY * 7, OUT);
        assert!(entry.exists(), "a fresh cache entry was removed");
        assert_eq!(failures, 0);
    }

    #[test]
    fn the_cache_sweep_reports_an_unreadable_cache_and_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let not_a_dir = dir.path().join("file");
        std::fs::write(&not_a_dir, b"x").unwrap();
        let failures = sweep_cache(&not_a_dir, &Config::default(), DAY * 7, OUT);
        assert!(not_a_dir.exists());
        assert_eq!(
            failures, 1,
            "an unreadable cache must count as a failed sweep"
        );
    }
}
