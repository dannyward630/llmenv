//! A run of `llmenv setup` on a machine that has a config: the merge (the default), and
//! the `--rescan` refresh that it uses (#2789).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{
    build_enumeration, count_project_entries, engine_handoff_prompt, install_setup_skill,
    probe_engines, write_enumeration_json,
};

/// Re-scan existing configs and refresh the enumeration JSON and the installed skill.
/// It keeps config.yaml, AGENTS.md, and the user's bundle files. It replaces the setup skill at
/// `bundles/base/skills/setup-llmenv/SKILL.md`, which llmenv owns and installs on every run.
pub(super) fn run_rescan(config_dir: &Path, no_launch: bool) -> Result<()> {
    if !config_dir.join("config.yaml").is_file() {
        anyhow::bail!(
            "No existing config found at {}. Run `llmenv setup` first.",
            config_dir.display()
        );
    }

    // --- Phase 1: Scan existing configs ---
    eprintln!();
    eprintln!("🔍 Re-scanning for existing tool configurations...");
    let home_dir = crate::env_var::utf8_var("HOME")?
        .filter(|h| !h.is_empty())
        .map(PathBuf::from);
    if let Some(ref h) = home_dir {
        let claude_settings = h.join(".claude").join("settings.json");
        if claude_settings.is_file() {
            eprintln!("  ✓ Found Claude Code settings");
        }
        let plugins = h.join(".claude").join("plugins.json");
        if plugins.is_file() {
            eprintln!("  ✓ Found Claude Code plugins");
        }
        let projects_dir = h.join(".claude").join("projects");
        if projects_dir.is_dir() {
            // Already confirmed to exist above, so a read_dir failure here is
            // a real error (e.g. EACCES), not the ordinary "doesn't exist"
            // case — worth logging rather than silently showing nothing
            // (#1180).
            let count = count_project_entries(&projects_dir);
            if count > 0 {
                eprintln!("  ✓ Found {count} Claude Code project config(s)");
            }
        }
    }

    // Probe engines
    let available = probe_engines();
    if available.is_empty() {
        eprintln!(
            "  No supported AI engines found on PATH ({}).",
            crate::adapter::registered_adapters()
                .iter()
                .map(|a| a.binary_name())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // --- Phase 2: Refresh enumeration JSON ---
    let enumeration = build_enumeration(&available, config_dir);
    write_enumeration_json(config_dir, &enumeration)?;
    eprintln!("✓ Environment snapshot refreshed in .llmenv-setup-state.json");

    // Re-install the setup skill
    install_setup_skill(config_dir)?;
    eprintln!("✓ Setup skill re-installed to bundles/base/skills/setup-llmenv/");

    // --- Engine handoff ---
    let skill_path = config_dir
        .join("bundles")
        .join("base")
        .join("skills")
        .join("setup-llmenv")
        .join("SKILL.md");
    let state_path = config_dir.join(".llmenv-setup-state.json");
    engine_handoff_prompt(&skill_path, &state_path, &available, no_launch)?;

    eprintln!();
    eprintln!("✅ Rescan complete! Config files were not modified.");

    Ok(())
}

/// What to do when `config.yaml` exists (#2789).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ExistingConfig {
    /// Keep every file, refresh the skill, and let the onboarding plan add to the config.
    Merge,
    /// Run the wizard again, which replaces `config.yaml`.
    Reset,
    Cancel,
}

/// Ask what to do with an existing config. The merge is the default, and the reset is a
/// deliberate second choice. `--no-launch` cannot ask, so it takes the merge, which keeps every
/// file.
pub(super) fn existing_config_choice(config_dir: &Path, no_launch: bool) -> Result<ExistingConfig> {
    if no_launch {
        return Ok(ExistingConfig::Merge);
    }
    anyhow::ensure!(
        super::is_interactive(),
        "{} exists, and `llmenv setup` needs a terminal to ask whether to keep it. Fix: run \
         `llmenv setup --rescan` to keep your files, or move config.yaml away to start again",
        config_dir.join("config.yaml").display()
    );
    let choice = dialoguer::Select::new()
        .with_prompt(format!("A config exists at {}.", config_dir.display()))
        .items([
            "Merge: keep every file, and suggest additions",
            "Reset: run setup again, and replace config.yaml",
            "Cancel",
        ])
        .default(0)
        .interact()
        .context("reading the choice for the existing config")?;
    Ok(match choice {
        0 => ExistingConfig::Merge,
        1 => ExistingConfig::Reset,
        _ => ExistingConfig::Cancel,
    })
}
