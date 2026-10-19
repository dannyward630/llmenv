//! The sweeps behind `llmenv clean` and the deprecated `llmenv doctor --gc`: the hashed cache,
//! then `state/tmp`, then session-log rotations, then old codebase-memory indexes (#2814, #2816,
//! #2917).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::Config;
use crate::materialize::cache::GcReport;
use crate::materialize::{cache, tmp_sweep};
use crate::paths;
use crate::session_log;

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

/// Run every sweep. All of them use `cache.cache_retention_hours` (default 168). A `null` value
/// turns all of them off (#2829).
pub(in crate::cli) fn run(cache_dir: &Path, config: &Config, out: GcOutput<'_>) {
    let (pass, warn) = (out.pass, out.warn);
    eprintln!("Running garbage collection...");
    let Some(retention) = config.cache.gc_retention() else {
        eprintln!(
            "{pass} GC skipped: cache.cache_retention_hours is null, so age-based GC is off. \
             Set it to a number of hours to turn GC on."
        );
        return;
    };
    match std::fs::metadata(cache_dir) {
        Ok(meta) if meta.permissions().readonly() => {
            eprintln!("{warn} GC of the cache skipped: cache directory is read-only");
        }
        Ok(_) => sweep_cache(cache_dir, config, retention, out),
        Err(e) => eprintln!("{warn} GC of the cache skipped: cannot stat cache directory: {e:#}"),
    }
    match paths::state_dir() {
        Ok(state_dir) => {
            sweep_state(&state_dir, retention, out);
            sweep_cbm_indexes(&state_dir, config, retention, out);
        }
        Err(e) => eprintln!(
            "{warn} GC of state/tmp and session-log rotations skipped: cannot resolve the state \
             directory: {e:#}"
        ),
    }
}

/// Sweep the hashed cache under `cache_dir`.
fn sweep_cache(cache_dir: &Path, config: &Config, retention: Duration, out: GcOutput<'_>) {
    match cache::gc(cache_dir, retention, config.cache.hashing, out.dry_run) {
        Ok(report) => {
            out.report("", &report);
            if out.dry_run {
                return;
            }
            let forgotten = forget_credentials_for(&report.removed);
            if forgotten > 0 {
                eprintln!(
                    "{} GC: dropped {forgotten} orphaned OAuth credential entries",
                    out.pass
                );
            }
        }
        Err(e) => eprintln!("{} GC failed: {e:#}", out.warn),
    }
}

/// Sweep the folders under `state_dir`: `state/tmp`, then the session-log rotations.
fn sweep_state(state_dir: &Path, retention: Duration, out: GcOutput<'_>) {
    sweep_state_tmp(state_dir, retention, out);
    prune_session_log(state_dir, retention, out);
}

/// Sweep `state/tmp`, which `TMPDIR` points at, with the same retention as the cache (#2814).
fn sweep_state_tmp(state_dir: &Path, retention: Duration, out: GcOutput<'_>) {
    match tmp_sweep::sweep_state_tmp(state_dir, retention, out.dry_run) {
        Ok(report) => out.report(" for state/tmp", &report),
        Err(e) => eprintln!("{} GC of state/tmp skipped: {e:#}", out.warn),
    }
}

/// Remove old index databases from `state/codebase-memory`, unless a configured `index_path`
/// names that folder as the live cache (#2817). A database that a process holds open is kept.
fn sweep_cbm_indexes(state_dir: &Path, config: &Config, retention: Duration, out: GcOutput<'_>) {
    let (pass, warn) = (out.pass, out.warn);
    let dir = state_dir.join("codebase-memory");
    match cache_use(config, &dir) {
        CacheUse::Unused => {}
        CacheUse::Live => {
            eprintln!(
                "{pass} GC of {} skipped: a features.codebase_memory index_path uses it",
                dir.display()
            );
            return;
        }
        CacheUse::Unknown(cause) => {
            eprintln!("{warn} GC of {} skipped: {cause}", dir.display());
            return;
        }
    }
    let in_use = match crate::materialize::open_files::open_paths() {
        Ok(in_use) => in_use,
        Err(e) => {
            eprintln!("{warn} GC of old codebase-memory indexes skipped: {e:#}");
            return;
        }
    };
    // lsof reports resolved paths, so the folder is resolved too before the paths are compared.
    let resolved = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
    let swept = crate::materialize::cbm_sweep::sweep_legacy_indexes(
        &resolved,
        retention,
        &in_use,
        out.dry_run,
    );
    match swept {
        Ok(report) => out.report(" for old codebase-memory indexes", &report),
        Err(e) => eprintln!("{warn} GC of old codebase-memory indexes skipped: {e:#}"),
    }
}

/// Drop the platform credential entry belonging to each cache folder GC just
/// deleted, and report how many went (#1057).
///
/// Keyed by folder path, so this only ever touches entries llmenv's own folders
/// owned — never the user's default `~/.claude` login. Deliberately confined to
/// the explicit `llmenv clean` path: removing credential entries as a side effect
/// of `export` would be the wrong default.
pub(in crate::cli) fn forget_credentials_for(removed: &[PathBuf]) -> usize {
    removed
        .iter()
        .filter(|path| {
            crate::auth::credentials::forget(path)
                .inspect_err(|e| {
                    tracing::debug!(
                        "could not drop credential entry for {}: {e}",
                        path.display()
                    );
                })
                .unwrap_or(false)
        })
        .count()
}

/// Whether a configured `index_path` names the codebase-memory folder.
#[derive(Debug, PartialEq, Eq)]
enum CacheUse {
    Unused,
    Live,
    /// An `index_path` cannot be expanded, so the answer is not known.
    Unknown(String),
}

fn cache_use(config: &Config, dir: &Path) -> CacheUse {
    let canonical_dir = std::fs::canonicalize(dir).ok();
    let raws = config
        .features
        .iter()
        .flat_map(|f| &f.codebase_memory)
        .filter_map(|cm| cm.index_path.as_deref());
    for raw in raws {
        let expanded = match paths::expand_tilde(raw) {
            Ok(expanded) => PathBuf::from(expanded),
            Err(e) => return CacheUse::Unknown(format!("cannot expand index_path {raw}: {e:#}")),
        };
        let same = expanded == dir
            || (canonical_dir.is_some() && std::fs::canonicalize(&expanded).ok() == canonical_dir);
        if same {
            return CacheUse::Live;
        }
    }
    CacheUse::Unused
}

/// Remove session-log rotations older than the retention (#2816). The active file is kept.
fn prune_session_log(state_dir: &Path, retention: Duration, out: GcOutput<'_>) {
    let path = session_log::file_sink::file_path_in(state_dir);
    match session_log::rotation::prune_rotations(&path, retention, out.dry_run) {
        Ok(removed) => {
            eprintln!(
                "{} GC: {} {} rotated session-log file(s)",
                out.pass,
                out.verb(),
                removed.len()
            );
            out.list(&removed);
        }
        Err(e) => eprintln!("{} GC of session-log rotations failed: {e:#}", out.warn),
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
            cache_use(&with(&dir.display().to_string()), &dir),
            CacheUse::Live
        );
        assert_eq!(cache_use(&with("/elsewhere/cbm"), &dir), CacheUse::Unused);
        assert_eq!(cache_use(&Config::default(), &dir), CacheUse::Unused);
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

        sweep_state(state.path(), DAY * 7, OUT);

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
        sweep_state(state.path(), DAY * 7, preview);

        assert!(rotated.exists(), "a dry run removed an old rotation");
    }

    #[test]
    fn the_state_sweep_with_no_tmp_folder_and_no_log_changes_nothing() {
        let state = tempfile::tempdir().unwrap();
        sweep_state(state.path(), DAY * 7, OUT);
        assert_eq!(std::fs::read_dir(state.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_cache_sweep_keeps_a_fresh_entry() {
        let cache = tempfile::tempdir().unwrap();
        let entry = cache.path().join("fresh");
        std::fs::create_dir(&entry).unwrap();
        sweep_cache(cache.path(), &Config::default(), DAY * 7, OUT);
        assert!(entry.exists(), "a fresh cache entry was removed");
    }

    #[test]
    fn the_cache_sweep_reports_an_unreadable_cache_and_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let not_a_dir = dir.path().join("file");
        std::fs::write(&not_a_dir, b"x").unwrap();
        sweep_cache(&not_a_dir, &Config::default(), DAY * 7, OUT);
        assert!(not_a_dir.exists());
    }
}
