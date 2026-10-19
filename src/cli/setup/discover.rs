//! Read the state that an engine already left in the home folder: Claude Code settings, plugins,
//! instruction files, and per-project settings. `llmenv setup` enumerates it for the setup skill.

use std::collections::BTreeMap;
use std::path::Path;

/// Read ~/.claude/settings.json if it exists.
pub(super) fn read_claude_settings(home: &Path) -> Option<serde_json::Value> {
    read_json_if_present(&home.join(".claude").join("settings.json"))
}

/// Read ~/.claude/plugins.json if it exists.
pub(super) fn read_claude_plugins(home: &Path) -> Option<serde_json::Value> {
    read_json_if_present(&home.join(".claude").join("plugins.json"))
}

/// The JSON in the regular file at `path`, or `None`. A missing file is quiet. A file that
/// cannot be checked, read, or parsed is reported, because `is_file` reads a permission error
/// as "absent".
fn read_json_if_present(path: &Path) -> Option<serde_json::Value> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("llmenv: cannot check {}: {e:#}", path.display());
            return None;
        }
    }
    let bytes = std::fs::read(path)
        .inspect_err(|e| eprintln!("llmenv: failed to read {}: {e:#}", path.display()))
        .ok()?;
    serde_json::from_slice(&bytes)
        .inspect_err(|e| eprintln!("llmenv: failed to parse {}: {e:#}", path.display()))
        .ok()
}

/// Read ~/.claude/claude.md if it exists.
pub(super) fn read_claude_md(home: &Path) -> Option<String> {
    let path = home.join(".claude").join("CLAUDE.md");
    if !path.is_file() {
        return None;
    }
    std::fs::read_to_string(&path)
        .inspect_err(|e| eprintln!("llmenv: failed to read {}: {e:#}", path.display()))
        .ok()
}

/// Read ~/.claude/gemini.md if it exists.
pub(super) fn read_gemini_md(home: &Path) -> Option<String> {
    let path = home.join(".claude").join("GEMINI.md");
    if !path.is_file() {
        return None;
    }
    std::fs::read_to_string(&path)
        .inspect_err(|e| eprintln!("llmenv: failed to read {}: {e:#}", path.display()))
        .ok()
}

/// Read per-project settings from ~/.claude/projects/*/settings.json.
pub(super) fn read_project_configs(home: &Path) -> BTreeMap<String, serde_json::Value> {
    let mut projects = BTreeMap::new();
    let projects_dir = home.join(".claude").join("projects");
    let entries = match std::fs::read_dir(&projects_dir) {
        Ok(e) => e,
        // NotFound is the ordinary "Claude Code never ran here" case, not
        // worth a log line (#1180) — anything else (e.g. EACCES) is.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return projects,
        Err(e) => {
            tracing::error!(
                error = %e,
                dir = %projects_dir.display(),
                "failed to read project configs"
            );
            return projects;
        }
    };
    for entry in entries.filter_map(|entry| {
        crate::paths::readable_entry(&projects_dir, "project config discovery", entry)
    }) {
        let name = entry.file_name().to_string_lossy().to_string();
        let settings_path = entry.path().join("settings.json");
        let Ok(bytes) = std::fs::read(&settings_path).inspect_err(|e| {
            eprintln!(
                "llmenv: failed to read project settings {}: {e:#}",
                settings_path.display()
            )
        }) else {
            continue;
        };
        let Ok(val) = serde_json::from_slice(&bytes).inspect_err(|e| {
            eprintln!(
                "llmenv: failed to parse project settings {}: {e:#}",
                settings_path.display()
            )
        }) else {
            continue;
        };
        projects.insert(name, val);
    }
    projects
}

/// Counts the readable entries of the project config directory. A read failure is logged and
/// counts as zero, so an unreadable directory is not silent (#1180, #2718).
pub(super) fn count_project_entries(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .inspect_err(|e| {
            tracing::error!(error = %e, dir = %dir.display(), "failed to read project configs");
        })
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    crate::paths::readable_entry(dir, "project config counts", entry)
                })
                .count()
        })
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use super::read_json_if_present;

    #[test]
    fn read_json_if_present_reads_a_file_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        assert_eq!(read_json_if_present(&file), None, "missing");
        fs::write(&file, r#"{"a":1}"#).unwrap();
        assert_eq!(
            read_json_if_present(&file),
            Some(serde_json::json!({"a": 1}))
        );
        // A path under a regular file fails the check with ENOTDIR, which is reported.
        assert_eq!(read_json_if_present(&file.join("child.json")), None);
        assert_eq!(
            read_json_if_present(dir.path()),
            None,
            "a folder is not a file"
        );
    }
}
