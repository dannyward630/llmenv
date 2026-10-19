//! Finds top-level config keys that `Config` does not read.
//!
//! `Config` has no `deny_unknown_fields`, so serde drops an unknown top-level key
//! without an error. This module reads the same file again and names those keys.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Context;

use crate::Config;

/// Top-level `Config` fields that skip serialization when unset, so a serialized default
/// `Config` does not list them. Add a field here when `Config` gains one more
/// `skip_serializing_if` field.
const SKIPPED_WHEN_UNSET: &[&str] = &["session_log", "statusline"];

/// Scope kinds that a misplaced top-level key often names. Each one belongs under `scope:`.
const SCOPE_KINDS: &[&str] = &["host", "user", "network", "content"];

/// The top-level keys that `Config` reads, taken from its own serialized shape.
fn known_keys() -> anyhow::Result<BTreeSet<String>> {
    let shape = serde_yaml::to_value(Config::default())
        .context("cannot list the top-level keys of Config")?;
    let mut keys: BTreeSet<String> = shape
        .as_mapping()
        .map(|mapping| {
            mapping
                .keys()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    keys.extend(SKIPPED_WHEN_UNSET.iter().map(|key| (*key).to_owned()));
    Ok(keys)
}

/// Text for a key that is not a string. No `Config` field has such a name.
fn non_string_key_text(key: &serde_yaml::Value) -> String {
    serde_yaml::to_string(key).map_or_else(|_| format!("{key:?}"), |text| text.trim().to_owned())
}

/// Returns the top-level keys in the config file at `path` that `Config` does not read.
///
/// A file with no top-level mapping returns an empty list. `Config::load` reports that case.
///
/// # Errors
/// Returns an error if the file cannot be read or is not valid YAML.
pub fn unknown_top_level_keys(path: &Path) -> anyhow::Result<Vec<String>> {
    let text = crate::read_config_text(path)?;
    let doc: serde_yaml::Value = serde_yaml::from_str(&text)
        .with_context(|| format!("failed to parse config file: {}", path.display()))?;
    let Some(mapping) = doc.as_mapping() else {
        return Ok(Vec::new());
    };
    let known = known_keys()?;
    Ok(mapping
        .keys()
        .filter_map(|key| match key.as_str() {
            Some(name) if known.contains(name) => None,
            Some(name) => Some(name.to_owned()),
            None => Some(non_string_key_text(key)),
        })
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
    use proptest::prelude::*;

    fn write_config(yaml: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.yaml"), yaml).unwrap();
        dir
    }

    fn unknown_in(yaml: &str) -> Vec<String> {
        let dir = write_config(yaml);
        unknown_top_level_keys(&dir.path().join("config.yaml")).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_fails_at_once_and_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        let err = format!("{:#}", unknown_top_level_keys(&path).unwrap_err());
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert!(err.contains("not a regular file"), "{err}");
    }

    #[test]
    fn a_file_over_the_cap_fails_and_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let size = usize::try_from(crate::MAX_CONFIG_FILE_BYTES).unwrap() + 1;
        std::fs::write(&path, vec![b'#'; size]).unwrap();
        let err = format!("{:#}", unknown_top_level_keys(&path).unwrap_err());
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert!(err.contains("more than"), "{err}");
    }

    #[test]
    fn skipped_when_unset_fields_are_known() {
        // These fields skip serialization when unset, so the derived shape cannot show them.
        let known = known_keys().unwrap();
        for key in SKIPPED_WHEN_UNSET {
            assert!(known.contains(*key), "missing `{key}`");
        }
        assert!(
            known.contains("plugin-collection"),
            "the renamed key must be listed"
        );
    }

    #[test]
    fn misplaced_scope_kind_is_reported() {
        assert_eq!(unknown_in("user:\n  - id: robo\n"), vec!["user".to_owned()]);
    }

    #[test]
    fn known_keys_are_not_reported() {
        let keys = unknown_in("cache: {}\nscope: {}\nbundle: []\nstatusline: {}\n");
        assert!(keys.is_empty(), "unexpected: {keys:?}");
    }

    #[test]
    fn a_nested_unknown_key_is_not_reported() {
        assert!(unknown_in("cache:\n  colour: red\n").is_empty());
    }

    #[test]
    fn a_non_string_key_is_reported() {
        assert_eq!(unknown_in("1: a\n"), vec!["1".to_owned()]);
    }

    #[test]
    fn empty_file_reports_no_keys() {
        assert!(unknown_in("").is_empty());
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

    proptest! {
        /// The report is exactly the unknown keys, in file order. Names start with `x_` so that
        /// YAML never reads one as a bool, a null, or a number.
        #[test]
        fn the_report_is_the_unknown_keys_in_file_order(
            known in prop::sample::subsequence(
                known_keys().unwrap().into_iter().collect::<Vec<_>>(),
                0..5,
            ),
            unknown in prop::collection::btree_set("x_[a-z]{1,8}", 0..6),
        ) {
            let mut yaml = String::new();
            for key in known.iter().chain(unknown.iter()) {
                yaml.push_str(&format!("{key}: 1\n"));
            }
            let expected: Vec<String> = unknown.into_iter().collect();
            prop_assert_eq!(unknown_in(&yaml), expected);
        }

        /// Any file content gives a list or an error, never a panic.
        #[test]
        fn arbitrary_file_content_never_panics(text in "\\PC{0,200}") {
            let dir = write_config(&text);
            let _ = unknown_top_level_keys(&dir.path().join("config.yaml"));
        }

        /// The message always carries the key. It names the `scope.` form for the four scope
        /// kinds only.
        #[test]
        fn the_message_names_the_key_and_the_scope_form_only_for_scope_kinds(
            key in prop_oneof![
                "[a-z]{1,10}",
                prop::sample::select(SCOPE_KINDS.to_vec()).prop_map(String::from),
            ],
        ) {
            let message = unknown_key_message(&key);
            let quoted = format!("`{key}`");
            let scope_form = format!("scope.{key}");
            prop_assert!(message.contains(&quoted));
            prop_assert_eq!(
                message.contains(&scope_form),
                SCOPE_KINDS.contains(&key.as_str()),
            );
        }
    }
}
