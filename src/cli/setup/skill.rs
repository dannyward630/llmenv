//! The `setup-llmenv` skill that `llmenv setup` installs for the engine hand-off.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::paths;

const SETUP_SKILL_SOURCE: &str = include_str!("../../../skills/setup-llmenv/SKILL.md");

/// Write the setup skill to the bundle's skills directory.
pub(super) fn install_setup_skill(config_dir: &Path) -> Result<PathBuf> {
    let skill_dir = config_dir
        .join("bundles")
        .join("base")
        .join("skills")
        .join("setup-llmenv");
    std::fs::create_dir_all(&skill_dir)
        .with_context(|| format!("creating skill dir {}", skill_dir.display()))?;

    let skill_path = skill_dir.join("SKILL.md");
    let state_path = config_dir.join(".llmenv-setup-state.json");
    let content = SETUP_SKILL_SOURCE
        .replace("{STATE_PATH}", &state_path.to_string_lossy())
        .replace("{config_dir}", &config_dir.to_string_lossy());
    paths::write_owner_only(&skill_path, content.as_bytes())
        .with_context(|| format!("writing skill {}", skill_path.display()))?;
    Ok(skill_path)
}
