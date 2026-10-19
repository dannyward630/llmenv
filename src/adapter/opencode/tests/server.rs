//! The environment and the server settings that keep an opencode client on its own server
//! (#2845).

use super::*;

#[test]
fn materialize_turns_off_the_shared_background_server() {
    let tmp = tempfile::tempdir().unwrap();
    let owned = OpencodeAdapter
        .materialize(&MergedManifest::default(), tmp.path())
        .unwrap();
    let raw = std::fs::read_to_string(tmp.path().join("service.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(doc["disabled"], true);
    assert!(owned.contains(&PathBuf::from("service.json")));
}

#[test]
fn env_vars_put_the_temp_folder_in_the_state_dir() {
    let cache = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let vars = OpencodeAdapter
        .env_vars(cache.path(), state.path())
        .unwrap();
    let tmp = state.path().join("tmp");
    assert!(
        tmp.is_dir(),
        "the temp folder must exist before a shell uses it"
    );
    for name in ["TMPDIR", "TMP", "TEMP"] {
        assert!(
            vars.contains(&(name.into(), tmp.to_string_lossy().into_owned())),
            "{name} missing: {vars:?}"
        );
    }
}

#[test]
fn env_vars_returns_opencode_config_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let vars = OpencodeAdapter.env_vars(tmp.path(), tmp.path()).unwrap();
    assert!(vars.contains(&(
        "OPENCODE_CONFIG_DIR".into(),
        tmp.path().to_string_lossy().into_owned()
    )));
}

#[test]
fn env_vars_turn_off_the_claude_skill_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let vars = OpencodeAdapter.env_vars(tmp.path(), tmp.path()).unwrap();
    assert!(vars.contains(&("OPENCODE_DISABLE_CLAUDE_CODE".into(), "1".into())));
}

#[test]
#[cfg(unix)]
fn env_vars_rejects_non_utf8_cache_dir() {
    use std::os::unix::ffi::OsStringExt;
    let non_utf8 = PathBuf::from(std::ffi::OsString::from_vec(vec![0xff, 0xfe]));
    let err = OpencodeAdapter
        .env_vars(&non_utf8, Path::new("/tmp"))
        .unwrap_err();
    assert!(err.to_string().contains("not valid UTF-8"));
}
