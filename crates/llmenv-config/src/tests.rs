use super::*;

#[cfg(unix)]
#[test]
fn load_fails_at_once_on_a_fifo_and_names_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    let made = std::process::Command::new("mkfifo")
        .arg(&p)
        .status()
        .unwrap();
    assert!(made.success());
    let err = format!("{:#}", Config::load(&p).unwrap_err());
    assert!(err.contains(&p.display().to_string()), "{err}");
    assert!(err.contains("not a regular file"), "{err}");
}

#[test]
fn the_config_cap_is_the_documented_4_mib() {
    assert_eq!(MAX_CONFIG_FILE_BYTES, 4_194_304);
}

#[test]
fn load_rejects_a_file_over_the_cap_and_names_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    let size = usize::try_from(MAX_CONFIG_FILE_BYTES).unwrap() + 1;
    std::fs::write(&p, vec![b'#'; size]).unwrap();
    let err = format!("{:#}", Config::load(&p).unwrap_err());
    assert!(err.contains(&p.display().to_string()), "{err}");
    assert!(err.contains("more than"), "{err}");
}

#[test]
fn load_of_a_missing_file_is_a_not_found_error() {
    let tmp = tempfile::tempdir().unwrap();
    let err = Config::load(&tmp.path().join("config.yaml")).unwrap_err();
    let io = err.downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(io.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn context_mode_source_is_pinned_not_floating() {
    // #496: the built-in context-mode marketplace source must not be an
    // unpinned floating HEAD ref — every regenerate would otherwise pull
    // whatever the upstream repo currently has, breaking reproducibility.
    assert!(
        CONTEXT_MODE_SOURCE
            .split_once('#')
            .is_some_and(|(_, r#ref)| !r#ref.is_empty()),
        "CONTEXT_MODE_SOURCE must carry a non-empty pinned #<tag> suffix: {CONTEXT_MODE_SOURCE}"
    );
}

#[test]
fn load_accepts_expanded_path() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    std::fs::write(&p, "cache: {}\n").unwrap();
    assert!(Config::load(&p).is_ok());
}

#[test]
fn session_log_absent_resolves_to_transcript_on() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    std::fs::write(&p, "cache: {}\n").unwrap();
    let cfg = Config::load(&p).unwrap();
    assert!(cfg.session_log.is_none());
    let resolved = cfg.session_log_resolved();
    assert!(!resolved.any_sink_wants(LogLevel::Debug));
    let t = resolved.transcript.as_ref().unwrap();
    assert!(t.enabled);
    assert_eq!(t.level, LogLevel::Info);
}

// #744: the pre-3.3 boolean shape used to be silently translated. Removed in
// 4.0 — but a bare "invalid type: boolean" from serde tells a user upgrading
// nothing about what to write instead, so the shape is still *detected*, only
// to produce an error that names its replacement.
#[test]
fn session_log_old_boolean_shape_is_rejected_with_migration_guidance() {
    for body in [
        "session_log:\n  file: true\n",
        "session_log:\n  transcript: false\n",
        "session_log:\n  verbose: true\n",
        "session_log:\n  file: true\n  transcript: false\n  verbose: true\n",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.yaml");
        std::fs::write(&p, body).unwrap();

        let err = format!("{:#}", Config::load(&p).unwrap_err());
        assert!(
            err.contains("no longer supported"),
            "error should say the shape is gone: {err}"
        );
        // The whole point of keeping a detection pass: the message has to
        // show the replacement, not just refuse the input.
        assert!(
            err.contains("enabled:"),
            "error should show the per-sink form: {err}"
        );
    }
}

// `verbose: true` meant Debug for both sinks; the replacement is a per-sink
// `level`. Naming it keeps the migration mechanical for anyone hitting this.
#[test]
fn old_shape_error_maps_verbose_onto_the_level_field() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    std::fs::write(&p, "session_log:\n  verbose: true\n").unwrap();
    let err = format!("{:#}", Config::load(&p).unwrap_err());
    assert!(err.contains("level:"), "verbose maps onto level: {err}");
}

// A mapping-valued `file`/`transcript` is the *new* shape and must not be
// caught by the old-shape guard — the detection keys on the boolean value,
// not on the field name.
#[test]
fn new_shape_keys_are_not_mistaken_for_the_old_boolean_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    std::fs::write(
        &p,
        "session_log:\n  file:\n    enabled: true\n    level: debug\n",
    )
    .unwrap();
    // The per-sink mapping form still parses.
    let r = Config::load(&p).unwrap().session_log_resolved();
    let f = r.file.as_ref().unwrap();
    assert!(f.enabled);
    assert_eq!(f.level, LogLevel::Debug);
}

#[test]
fn session_log_new_shape_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    std::fs::write(
        &p,
        "session_log:\n  file:\n    enabled: true\n    level: trace\n  transcript:\n    enabled: true\n    level: info\n",
    )
    .unwrap();
    let r = Config::load(&p).unwrap().session_log_resolved();
    let f = r.file.as_ref().unwrap();
    assert!(f.enabled);
    assert_eq!(f.level, LogLevel::Trace);
    let t = r.transcript.as_ref().unwrap();
    assert!(t.enabled);
    assert_eq!(t.level, LogLevel::Info);
}

#[test]
fn session_log_bare_string_is_rejected_with_migration_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    std::fs::write(&p, "session_log: /tmp/session.jsonl\n").unwrap();
    let err = Config::load(&p).unwrap_err().to_string();
    // The full chain mentions the field path; the source carries the hint.
    let chain = format!("{:#}", Config::load(&p).unwrap_err());
    assert!(chain.contains("session_log") || err.contains("session_log"));
    assert!(
        chain.contains("file: true"),
        "error shows the migration: {chain}"
    );
}

#[test]
fn load_expands_tilde_path() {
    // #1910: `load` used to only debug_assert the caller pre-expanded `~`,
    // so a release build silently mis-resolved a tilde path instead of
    // erroring. It must now expand `~` itself.
    let home = std::env::var("HOME").unwrap();
    let dir = tempfile::Builder::new()
        .prefix(".llmenv-config-test-")
        .tempdir_in(&home)
        .unwrap();
    let p = dir.path().join("config.yaml");
    std::fs::write(&p, "cache: {}\n").unwrap();
    let dir_name = dir.path().file_name().unwrap().to_str().unwrap();
    let tilde_path = format!("~/{dir_name}/config.yaml");
    assert!(Config::load(Path::new(&tilde_path)).is_ok());
}

#[test]
fn load_expands_tilde_in_root_codebase_memory_index_path() {
    // #2651: a root-level entry is not merged, so `load` must expand it itself.
    let home = std::env::var("HOME").unwrap();
    let dir = tempfile::Builder::new()
        .prefix(".llmenv-config-test-")
        .tempdir_in(&home)
        .unwrap();
    let p = dir.path().join("config.yaml");
    let yaml = "features:\n  codebase_memory:\n    - when: [proj]\n      index_path: ~/cache/cbm\n";
    std::fs::write(&p, yaml).unwrap();
    let cfg = Config::load(&p).unwrap();
    let cm = &cfg.features.unwrap().codebase_memory[0];
    let index_path = cm.index_path.as_deref().unwrap();
    assert!(
        !index_path.starts_with('~'),
        "tilde must be expanded: {index_path}"
    );
    assert!(
        index_path.ends_with("/cache/cbm"),
        "suffix kept: {index_path}"
    );
}

// macOS (APFS/HFS+) rejects invalid-UTF-8 filenames at the syscall level, so this
// property is only observable on Linux, where filenames are arbitrary bytes.
#[cfg(target_os = "linux")]
#[test]
fn load_does_not_mangle_a_non_tilde_non_utf8_path() {
    // #1910 follow-up: expansion must only touch tilde-prefixed paths.
    // Round-tripping every path through `to_string_lossy` would corrupt
    // a non-UTF-8 path that never needed expansion in the first place.
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let tmp = tempfile::tempdir().unwrap();
    let mut bytes = tmp.path().join("").into_os_string().into_vec();
    bytes.extend_from_slice(b"caf\xe9.yaml"); // invalid UTF-8 byte 0xe9
    let p = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&bytes));
    std::fs::write(&p, "cache: {}\n").unwrap();
    assert!(Config::load(&p).is_ok());
}

#[cfg(target_os = "linux")]
#[test]
fn load_expands_tilde_without_mangling_non_utf8_bytes_after_it() {
    // A tilde path also needs the non-UTF-8 guarantee: only `~` itself
    // (always valid UTF-8 -- $HOME) goes through string handling, so a
    // non-UTF-8 byte in the rest of the path survives untouched.
    use std::os::unix::ffi::OsStrExt;

    let home = std::env::var("HOME").unwrap();
    let dir = tempfile::Builder::new()
        .prefix(".llmenv-config-test-")
        .tempdir_in(&home)
        .unwrap();

    let filename_bytes = b"caf\xe9.yaml"; // invalid UTF-8 byte 0xe9
    let filename = std::ffi::OsStr::from_bytes(filename_bytes);
    std::fs::write(dir.path().join(filename), "cache: {}\n").unwrap();

    let dir_name = dir.path().file_name().unwrap().as_bytes();
    let mut tilde_bytes = b"~/".to_vec();
    tilde_bytes.extend_from_slice(dir_name);
    tilde_bytes.push(b'/');
    tilde_bytes.extend_from_slice(filename_bytes);
    let tilde_path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&tilde_bytes));
    assert!(Config::load(&tilde_path).is_ok());
}

#[test]
fn mcp_permissions_rejects_invalid_action_value() {
    // #946: `features.<name>.mcp_permissions` values must be one of
    // allow|ask|deny — anything else is a clear config error.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    std::fs::write(
        &p,
        "cache: {}\n\
         features:\n\
         \x20\x20context_mode:\n\
         \x20\x20\x20\x20enabled: true\n\
         \x20\x20\x20\x20mcp_permissions:\n\
         \x20\x20\x20\x20\x20\x20read_only: maybe\n",
    )
    .unwrap();
    let err = Config::load(&p).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("failed to parse config file"),
        "expected a config-parse error, got: {msg}"
    );
}

#[test]
fn mcp_permissions_accepts_valid_action_values() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.yaml");
    std::fs::write(
        &p,
        "cache: {}\n\
         features:\n\
         \x20\x20context_mode:\n\
         \x20\x20\x20\x20enabled: true\n\
         \x20\x20\x20\x20mcp_permissions:\n\
         \x20\x20\x20\x20\x20\x20read_only: allow\n\
         \x20\x20\x20\x20\x20\x20mutation: allow\n\
         \x20\x20\x20\x20\x20\x20destructive: deny\n",
    )
    .unwrap();
    let cfg = Config::load(&p).unwrap();
    let perms = cfg
        .features
        .unwrap()
        .context_mode
        .unwrap()
        .mcp_permissions
        .unwrap();
    assert_eq!(perms.read_only, Some(McpPermissionAction::Allow));
    assert_eq!(perms.mutation, Some(McpPermissionAction::Allow));
    assert_eq!(perms.destructive, Some(McpPermissionAction::Deny));
}

use proptest::prelude::*;
proptest! {
    /// #1910: an arbitrary non-tilde-prefixed filename must load
    /// unchanged — the tilde branch in `load` must never fire on it, and
    /// no round trip may alter which file gets read.
    #[test]
    fn prop_load_non_tilde_filename_reads_the_exact_file(name in "[a-zA-Z0-9_-]{1,20}") {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(format!("{name}.yaml"));
        std::fs::write(&p, "cache: {}\n").unwrap();
        prop_assert!(Config::load(&p).is_ok());
    }

    /// #1910: an arbitrary tilde-prefixed path resolves under `$HOME`,
    /// for any legal file/dir name component after the `~/`.
    #[test]
    fn prop_load_tilde_path_resolves_under_home(name in "[a-zA-Z0-9_-]{1,20}") {
        let home = std::env::var("HOME").unwrap();
        let dir = tempfile::Builder::new()
            .prefix(".llmenv-config-prop-test-")
            .tempdir_in(&home)
            .unwrap();
        let p = dir.path().join(format!("{name}.yaml"));
        std::fs::write(&p, "cache: {}\n").unwrap();
        let dir_name = dir.path().file_name().unwrap().to_str().unwrap();
        let tilde_path = format!("~/{dir_name}/{name}.yaml");
        prop_assert!(Config::load(Path::new(&tilde_path)).is_ok());
    }

    /// A tilde that doesn't lead the path (e.g. mid-string) is a literal
    /// character, not an expansion trigger — `strip_prefix("~")` on the
    /// whole path only matches a *leading* `~` component.
    #[test]
    fn prop_load_mid_path_tilde_is_not_expanded(name in "[a-zA-Z0-9_-]{1,20}") {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(format!("{name}~extra.yaml"));
        std::fs::write(&p, "cache: {}\n").unwrap();
        prop_assert!(Config::load(&p).is_ok());
    }
}
