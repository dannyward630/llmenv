//! Finds top-level config keys that `Config` does not read.
//!
//! `Config` has no `deny_unknown_fields`, so serde drops an unknown top-level key
//! without an error. This module reads the same file again and names those keys.

use std::path::Path;

use anyhow::Context;

/// Top-level keys that `Config` reads.
///
/// Kept by hand. Two fields skip serialization, so a serialized default `Config`
/// cannot list every key. The test below checks this list against the struct.
const TOP_LEVEL_KEYS: &[&str] = &[
    "disabled_engines",
    "cache",
    "scope",
    "capabilities",
    "native",
    "bundle",
    "mcp",
    "lsp",
    "skills",
    "output_styles",
    "features",
    "marketplace",
    "plugin-collection",
    "state",
    "host",
    "init",
    "session_log",
    "statusline",
];

/// Scope kinds that a misplaced top-level key often names. Each one belongs under `scope:`.
const SCOPE_KINDS: &[&str] = &["host", "user", "network", "content"];

/// Returns the top-level keys in the config file at `path` that `Config` does not read.
///
/// A file with no top-level mapping returns an empty list. `Config::load` reports that case.
///
/// # Errors
/// Returns an error if the file cannot be read or is not valid YAML.
pub fn unknown_top_level_keys(path: &Path) -> anyhow::Result<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&text)
        .with_context(|| format!("failed to parse config file: {}", path.display()))?;
    let Some(mapping) = doc.as_mapping() else {
        return Ok(Vec::new());
    };
    Ok(mapping
        .keys()
        .filter_map(serde_yaml::Value::as_str)
        .filter(|key| !TOP_LEVEL_KEYS.contains(key))
        .map(str::to_owned)
        .collect())
}

/// Builds the advisory text for one unknown top-level key.
///
/// A scope kind gets the `scope:` fix, because that is the usual mistake.
#[must_use]
pub fn unknown_key_message(key: &str) -> String {
    if SCOPE_KINDS.contains(&key) {
        format!("top-level key `{key}` is ignored; move it under `scope.{key}`")
    } else {
        format!("top-level key `{key}` is ignored; llmenv does not read it")
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::*;
    use crate::Config;

    fn write_config(yaml: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.yaml"), yaml).unwrap();
        dir
    }

    #[test]
    fn every_serialized_default_key_is_listed() {
        let value = serde_yaml::to_value(Config::default()).unwrap();
        for key in value
            .as_mapping()
            .unwrap()
            .keys()
            .filter_map(serde_yaml::Value::as_str)
        {
            assert!(
                TOP_LEVEL_KEYS.contains(&key),
                "Config serializes `{key}` but TOP_LEVEL_KEYS does not list it"
            );
        }
    }

    #[test]
    fn skip_serialized_keys_are_listed() {
        // These fields skip serialization when unset, so the test above cannot see them.
        for key in ["session_log", "statusline", "plugin-collection"] {
            assert!(TOP_LEVEL_KEYS.contains(&key), "missing `{key}`");
        }
    }

    #[test]
    fn misplaced_scope_kind_is_reported() {
        let dir = write_config("user:\n  - id: robo\n");
        let keys = unknown_top_level_keys(&dir.path().join("config.yaml")).unwrap();
        assert_eq!(keys, vec!["user".to_owned()]);
    }

    #[test]
    fn known_keys_are_not_reported() {
        let dir = write_config("cache: {}\nscope: {}\nbundle: []\n");
        let keys = unknown_top_level_keys(&dir.path().join("config.yaml")).unwrap();
        assert!(keys.is_empty(), "unexpected: {keys:?}");
    }

    #[test]
    fn empty_file_reports_no_keys() {
        let dir = write_config("");
        let keys = unknown_top_level_keys(&dir.path().join("config.yaml")).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn missing_file_is_an_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.yaml");
        let err = unknown_top_level_keys(&path).unwrap_err().to_string();
        assert!(err.contains("absent.yaml"), "error lacks the path: {err}");
    }

    #[test]
    fn scope_kind_message_names_the_scope_form() {
        assert_eq!(
            unknown_key_message("user"),
            "top-level key `user` is ignored; move it under `scope.user`"
        );
    }

    #[test]
    fn other_key_message_says_llmenv_does_not_read_it() {
        assert_eq!(
            unknown_key_message("colour"),
            "top-level key `colour` is ignored; llmenv does not read it"
        );
    }
}
