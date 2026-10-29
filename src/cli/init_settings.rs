//! The settings import of `llmenv init`: pick keys from `~/.claude/settings.json` and seed them.

use std::path::{Path, PathBuf};

use anyhow::Context as _;

use crate::config::Config;
use crate::paths;

use crate::onboarding::config_splice::{Addition, splice};

/// Interactive settings import for `llmenv init` (#172).
///
/// Reads `~/.claude/settings.json`, presents the non-owned keys as a checkbox
/// list, and stores selected keys in `config.yaml` under `init.seeded_settings`.
pub(super) fn run_init_settings_prompt(config_path: &Path) -> anyhow::Result<()> {
    use crate::adapter::claude_code::LLMENV_OWNED_SETTINGS_KEYS;
    use dialoguer::MultiSelect;

    let global_settings = PathBuf::from(paths::expand_tilde("~/.claude")?).join("settings.json");
    // #918: NotFound → no settings to import (silent); a permission error
    // surfaces a warning instead of an exists() stat masking it as absent.
    let bytes = match std::fs::read(&global_settings) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            eprintln!(
                "warning: could not read {} — skipping settings import: {e}",
                global_settings.display()
            );
            return Ok(());
        }
    };
    let doc: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "warning: {} is not valid JSON — skipping settings import: {e}",
                global_settings.display()
            );
            return Ok(());
        }
    };
    let Some(obj) = doc.as_object() else {
        return Ok(());
    };
    let candidates: Vec<(&String, &serde_json::Value)> = obj
        .iter()
        .filter(|(k, _)| !LLMENV_OWNED_SETTINGS_KEYS.contains(&k.as_str()))
        .collect();
    if candidates.is_empty() {
        return Ok(());
    }

    let labels: Vec<String> = candidates
        .iter()
        .map(|(k, v)| format!("{k} = {v}"))
        .collect();
    let chosen = MultiSelect::new()
        .with_prompt(
            "Select settings from ~/.claude/settings.json to seed into new folders \
             (space to toggle, enter to confirm)",
        )
        .items(&labels)
        .interact_opt()
        .map_err(|e| anyhow::anyhow!("settings prompt failed: {e}"))?;

    let Some(indices) = chosen else {
        return Ok(());
    };
    if indices.is_empty() {
        return Ok(());
    }
    let count = indices.len();

    // Check that the config parses. Then splice the keys in as text. Re-serializing the whole
    // document would drop the comments of the template (#2964).
    Config::load(config_path)?;
    let old = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading config {}", config_path.display()))?;
    let picks: Vec<(&String, &serde_json::Value)> =
        indices.into_iter().map(|idx| candidates[idx]).collect();
    let new_text = with_seeded_settings(&old, &picks)?;
    paths::write_user_file_atomic(config_path, new_text.as_bytes())
        .with_context(|| format!("writing config {}", config_path.display()))?;
    eprintln!("✓ {count} setting(s) added to init.seeded_settings in config.yaml");
    Ok(())
}

/// The config text with the picked keys added under `init.seeded_settings`. The keys are
/// spliced in as text, so the comments of `old` stay (#2964).
fn with_seeded_settings(
    old: &str,
    picks: &[(&String, &serde_json::Value)],
) -> anyhow::Result<String> {
    let mut keys = serde_yaml::Mapping::new();
    for (key, val) in picks {
        let yaml_val = serde_yaml::to_value(val).context("serializing a seeded setting")?;
        keys.insert(serde_yaml::Value::from(key.as_str()), yaml_val);
    }
    let addition = Addition::Keys {
        key: "init",
        sub: Some("seeded_settings"),
        keys,
    };
    let (new_text, _diff) = splice(old, &[addition])?;
    Ok(new_text)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code")]
mod tests {
    use super::with_seeded_settings;

    #[test]
    fn seeded_settings_keep_the_template_comments() {
        let old = "# guidance for the user\nlog_level: info\n";
        let key = "model".to_string();
        let value = serde_json::json!("on");

        let text = with_seeded_settings(old, &[(&key, &value)]).expect("splice");

        assert!(text.contains("# guidance for the user"), "{text}");
        assert!(text.contains("model: on"), "{text}");
    }
}
