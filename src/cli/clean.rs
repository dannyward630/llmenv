//! `llmenv clean`: the one cleanup command (#2917). It removes the stale cache folders that
//! `prune` removed, then runs the sweeps that `doctor --gc` ran. `prune` and `doctor --gc` are
//! deprecated forms of it.

use std::path::{Path, PathBuf};

use anyhow::Context;

use super::doctor::cache_gc::{self, GcOutput};
use crate::config::Config;
use crate::paths;

/// The line that `llmenv prune` prints before it runs `clean`.
pub(super) const PRUNE_DEPRECATED: &str = "llmenv prune is deprecated; use llmenv clean";
/// The line that `llmenv doctor --gc` prints before its sweeps.
pub(super) const DOCTOR_GC_DEPRECATED: &str = "llmenv doctor --gc is deprecated; use llmenv clean";

/// The flags of `llmenv clean`, which the deprecated `llmenv prune` shares.
#[derive(Debug, clap::Args)]
pub(super) struct CleanArgs {
    /// Remove ALL cache folders unconditionally (next export re-materializes all environments)
    #[arg(long)]
    all: bool,
    /// Remove ONLY current-version cache folders older than this duration (e.g., "14d", "1w")
    #[arg(long)]
    older_than: Option<String>,
    /// List every item that a real run removes, and remove nothing
    #[arg(long)]
    dry_run: bool,
    /// Remove the shared plugin cache directory for all adapters (state/<adapter>/state/plugins/)
    #[arg(long)]
    plugin_cache: bool,
}

/// Run `llmenv clean`: the cache-folder cleanup, then every age-based sweep.
///
/// # Errors
/// Fails on a bad flag combination, an unreadable config, or a cache entry that could not be
/// removed. The sweeps still run after a cache-folder failure, and the error comes after them.
pub(super) fn run_clean(args: &CleanArgs, use_color: bool) -> anyhow::Result<()> {
    if args.all && args.older_than.is_some() {
        anyhow::bail!("--all and --older-than are mutually exclusive");
    }
    let config = Config::load(&paths::config_path()?)?;
    let cache_dir = PathBuf::from(paths::expand_tilde(&config.cache.cache_dir)?);
    let pruned = run_prune(args, &config, &cache_dir);
    let (pass, warn) = (
        super::doctor_pass(use_color),
        super::doctor_warning(use_color),
    );
    let out = GcOutput {
        pass: &pass,
        warn: &warn,
        dry_run: args.dry_run,
    };
    cache_gc::run(&cache_dir, &config, out);
    pruned
}

/// Run the deprecated `llmenv prune`: print the notice, then run `clean` with the same flags.
///
/// # Errors
/// The same as [`run_clean`].
pub(super) fn run_prune_alias(args: &CleanArgs, use_color: bool) -> anyhow::Result<()> {
    eprintln!("{PRUNE_DEPRECATED}");
    run_clean(args, use_color)
}

/// The `prune` share of `clean`: stale cache folders and, with `--plugin-cache`, the shared
/// plugin caches.
fn run_prune(args: &CleanArgs, config: &Config, cache_dir: &Path) -> anyhow::Result<()> {
    use crate::materialize::cache::PruneMode;
    let CleanArgs {
        all,
        older_than,
        dry_run,
        plugin_cache,
    } = args;
    let (all, dry_run, plugin_cache) = (*all, *dry_run, *plugin_cache);

    // Validate flag combinations
    if all && older_than.is_some() {
        anyhow::bail!("--all and --older-than are mutually exclusive");
    }

    let mode = if all {
        PruneMode::All
    } else if let Some(duration_str) = &older_than {
        let dur = humantime::parse_duration(duration_str)
            .with_context(|| format!("failed to parse --older-than duration: {duration_str}"))?;
        PruneMode::OlderThan(dur)
    } else {
        // Default: clean stale (version-mismatched) folders + orphaned *.tmp.
        PruneMode::StaleOnly
    };

    // --plugin-cache: remove shared plugin cache dirs. Independent of other flags.
    let mut plugin_removed: Vec<PathBuf> = Vec::new();
    let mut plugin_failed: Vec<PathBuf> = Vec::new();
    if plugin_cache {
        for adapter in crate::adapter::registered_adapters() {
            let plugins_dir = cache_dir
                .join(adapter.name())
                .join(crate::materialize::state::STATE_DIR_NAME)
                .join("plugins");
            if dry_run {
                // Report as removed but don't actually delete.
                plugin_removed.push(plugins_dir);
            } else {
                match std::fs::remove_dir_all(&plugins_dir) {
                    Ok(()) => plugin_removed.push(plugins_dir),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        // Already gone — not a failure.
                        plugin_removed.push(plugins_dir);
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %plugins_dir.display(),
                            error = %e,
                            "failed to remove plugin cache"
                        );
                        plugin_failed.push(plugins_dir);
                    }
                }
            }
        }
    }

    // In normal mode the current generation dir (e.g. "1.2") has no
    // `{VERSION_TAG}-` prefix, so StaleOnly must be told its name or it would
    // sweep the live config dir. Loose mode has no version axis (every shape is
    // current) and strict mode is identified by the prefix test — both pass None.
    let current_version = match config.cache.hashing {
        crate::config::HashingMode::Normal => Some(crate::materialize::cache::version_major()),
        crate::config::HashingMode::Loose | crate::config::HashingMode::Strict => None,
    };

    // prune runs per adapter subdirectory: the generation/VERSION_TAG folders
    // live under `<cache_dir>/<adapter>/`, not directly under `cache_dir`.
    let mut removed: Vec<PathBuf> = Vec::new();
    let mut failed: Vec<PathBuf> = Vec::new();
    let mut kept: usize = 0;
    for adapter in crate::adapter::registered_adapters() {
        let report = crate::materialize::cache::prune(
            &cache_dir.join(adapter.name()),
            mode,
            config.cache.hashing,
            current_version.as_deref(),
            dry_run,
        )?;
        removed.extend(report.removed);
        failed.extend(report.failed);
        kept += report.kept;
    }

    let verb = if dry_run { "would remove" } else { "removed" };
    for p in &removed {
        eprintln!("  {verb}: {}", p.display());
    }
    for p in &failed {
        eprintln!(
            "  could not check or remove (cause logged above): {}",
            p.display()
        );
    }
    eprintln!(
        "prune complete: {} {} entry(ies), kept {}",
        verb,
        removed.len(),
        kept
    );
    if !failed.is_empty() {
        eprintln!(
            "  {} entry(ies) could not be checked or removed",
            failed.len()
        );
    }
    // Drop each removed folder's platform credential entry. The gc share already
    // does this; a prune deleting folders without it is how orphaned keychain items
    // accumulate in the first place (#1057).
    if !dry_run && !removed.is_empty() {
        let forgotten = cache_gc::forget_credentials_for(&removed);
        if forgotten > 0 {
            eprintln!("  dropped {forgotten} orphaned OAuth credential entry(ies)");
        }
    }

    // Report plugin cache results separately (verb is set above).
    if plugin_cache {
        for p in &plugin_removed {
            eprintln!("  {verb} plugin cache: {}", p.display());
        }
        for p in &plugin_failed {
            eprintln!("  failed to remove plugin cache: {}", p.display());
        }
        eprintln!(
            "plugin cache prune: {} {} entry(ies)",
            verb,
            plugin_removed.len(),
        );
    }

    // #1346: same contract as `regenerate` — a prune that could not remove
    // everything it was asked to is not a success, even though the per-entry
    // failures were printed above. Returning `Ok(())` here meant a scripted
    // `llmenv prune` saw exit 0 and moved on with the cache still occupied.
    //
    // #1372: this originally covered only `plugin_failed`, so a cache entry that
    // couldn't be removed printed "could not be removed" and still exited 0.
    // Both lists gate the exit status now.
    if !failed.is_empty() {
        anyhow::bail!(
            "{} cache entry(ies) could not be removed — see the failures above",
            failed.len()
        );
    }
    if plugin_cache && !plugin_failed.is_empty() {
        anyhow::bail!(
            "{} plugin cache entry(ies) could not be removed — see the failures above",
            plugin_failed.len()
        );
    }

    Ok(())
}
