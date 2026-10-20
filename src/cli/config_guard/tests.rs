use super::*;
use proptest::prelude::*;

#[cfg(unix)]
#[test]
#[expect(clippy::unwrap_used, reason = "test code")]
fn create_refusing_symlink_does_not_write_through_a_marker_link() {
    let dir = tempfile::TempDir::new().unwrap();
    let target = dir.path().join("victim");
    let marker = dir.path().join(".transcripts-migrated");
    std::fs::write(&target, b"keep").unwrap();
    std::os::unix::fs::symlink(&target, &marker).unwrap();
    assert!(
        crate::paths::create_refusing_symlink(&marker).is_err(),
        "a symlink marker must be refused"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}

#[test]
fn test_is_within_cache_basic() {
    let cache_root = std::path::Path::new("/home/user/.cache/llmenv");
    assert!(crate::util::is_within(
        cache_root,
        "/home/user/.cache/llmenv/config"
    ));
    assert!(crate::util::is_within(
        cache_root,
        "/home/user/.cache/llmenv/data/file.txt"
    ));
    assert!(!crate::util::is_within(
        cache_root,
        "/home/user/.config/llmenv/config.yaml"
    ));
    assert!(!crate::util::is_within(
        cache_root,
        "/home/user/.cache/llmenv-adjacent/file"
    ));
}

#[test]
fn test_is_within_cache_empty_path() {
    let cache_root = std::path::Path::new("/cache");
    assert!(!crate::util::is_within(cache_root, ""));
}

#[test]
fn test_is_within_cache_dot_dot_escape() {
    let cache_root = std::path::Path::new("/cache/llmenv");
    // After normalization, /cache/llmenv/../../../etc/passwd → /etc/passwd,
    // which does not start with /cache/llmenv.
    let escaped = "/cache/llmenv/../../../etc/passwd";
    assert!(!crate::util::is_within(cache_root, escaped));
}

#[test]
fn extract_hook_file_path_from_edit_fixture() {
    let raw = crate::test_fixtures::load_hook_payload_raw("edit.json");
    assert_eq!(
        extract_hook_file_path(&raw).as_deref(),
        Some("/path/to/file.rs")
    );
}

#[test]
fn extract_hook_file_path_from_write_fixture() {
    let raw = crate::test_fixtures::load_hook_payload_raw("write.json");
    assert_eq!(
        extract_hook_file_path(&raw).as_deref(),
        Some("/path/to/file.rs")
    );
}

#[test]
fn extract_hook_file_path_from_bash_fixture_is_none() {
    // Bash's tool_input has no file_path field.
    let raw = crate::test_fixtures::load_hook_payload_raw("bash.json");
    assert_eq!(extract_hook_file_path(&raw), None);
}

#[test]
fn extract_hook_file_path_missing_tool_input_is_none() {
    assert_eq!(extract_hook_file_path(r#"{"tool_name": "SomeTool"}"#), None);
}

#[test]
fn extract_hook_file_path_from_malformed_json_is_none() {
    assert_eq!(extract_hook_file_path("not valid json{"), None);
}

#[test]
fn extract_hook_file_path_from_empty_stdin_is_none() {
    assert_eq!(extract_hook_file_path(""), None);
}

// Property-based tests
proptest! {
    #[test]
    fn prop_within_cache_reflexive(ref cache_root in r"/[a-z/]+") {
        let path = std::path::Path::new(cache_root);
        prop_assert!(crate::util::is_within(path, cache_root));
    }

    #[test]
    fn prop_within_cache_child_paths(ref cache_root in r"/[a-z/]+", ref suffix in r"[a-z0-9]+") {
        let path = std::path::Path::new(cache_root);
        let child = format!("{}/{}", cache_root, suffix);
        prop_assert!(crate::util::is_within(path, &child));
    }

    #[test]
    fn prop_sibling_paths_not_within(ref cache_root in r"/cache/[a-z]+") {
        let path = std::path::Path::new(cache_root);
        // Adjacent directory with similar prefix should not match
        if let Some(base) = cache_root.rfind('/') {
            let sibling = format!("{}{}", &cache_root[..=base], "other");
            prop_assert!(!crate::util::is_within(path, &sibling));
        }
    }

    #[test]
    fn prop_no_panic_on_arbitrary_strings(ref cache_root in r"/[a-z/]*", ref path_str in ".*") {
        let path = std::path::Path::new(cache_root);
        // Should never panic, even on malformed paths
        let _ = crate::util::is_within(path, path_str);
    }
}

#[test]
fn config_dir_var_unset_selects_the_default_root() {
    assert_eq!(
        config_dir_from_var("CLAUDE_CONFIG_DIR", Err(std::env::VarError::NotPresent)),
        Ok(None)
    );
}

#[test]
fn config_dir_var_set_is_used_as_given() {
    let dir = "/opt/claude-config".to_string();
    assert_eq!(
        config_dir_from_var("CLAUDE_CONFIG_DIR", Ok(dir.clone())),
        Ok(Some(dir))
    );
}

#[cfg(unix)]
#[test]
fn config_dir_var_non_unicode_names_the_variable() {
    use std::os::unix::ffi::OsStringExt as _;
    let raw = std::ffi::OsString::from_vec(vec![0xff]);
    let msg = config_dir_from_var(
        "CLAUDE_CONFIG_DIR",
        Err(std::env::VarError::NotUnicode(raw)),
    )
    .expect_err("a non-UTF-8 config dir must not select the default root");
    assert!(msg.contains("CLAUDE_CONFIG_DIR"), "{msg}");
}

#[test]
fn opencode_reads_its_own_config_dir_variable() {
    assert_eq!(
        engine_config_var("opencode"),
        ("OPENCODE_CONFIG_DIR", "opencode")
    );
    assert_eq!(
        engine_config_var("claude_code"),
        ("CLAUDE_CONFIG_DIR", "claude-code")
    );
    let msg = config_dir_from_var(
        "OPENCODE_CONFIG_DIR",
        Err(std::env::VarError::NotUnicode(std::ffi::OsString::new())),
    )
    .unwrap_err();
    assert!(msg.contains("OPENCODE_CONFIG_DIR"), "{msg}");
}

#[test]
fn a_patch_call_names_each_file_against_its_cwd() {
    let payload = serde_json::json!({
        "tool_name": "patch",
        "cwd": "/repo",
        "tool_input": { "patchText": "*** Begin Patch\n*** Update File: src/a.rs\n*** Add File: /abs/b.rs\n*** End Patch" },
    });
    assert_eq!(
        extract_hook_file_paths(&payload.to_string()),
        ["/repo/src/a.rs", "/abs/b.rs"]
    );
    let edit = serde_json::json!({ "tool_name": "Edit", "tool_input": { "file_path": "/x" } });
    assert_eq!(extract_hook_file_paths(&edit.to_string()), ["/x"]);
}
