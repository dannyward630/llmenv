//! Tests for importing an existing `.claude.json` into the auth cache.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test fails loudly when its setup fails"
)]

use std::path::Path;

use super::import_auth_from_file;

const UUID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

/// Writes `contents` to `dir/.claude.json` and returns its path.
fn write_source(dir: &Path, contents: &str) -> std::path::PathBuf {
    let path = dir.join(".claude.json");
    std::fs::write(&path, contents).unwrap();
    path
}

/// True when some file under `dir` holds `needle`. The cache layout is private to `auth`,
/// so the test looks for the account in whatever file it wrote.
fn any_file_contains(dir: &Path, needle: &str) -> bool {
    std::fs::read_dir(dir).unwrap().any(|entry| {
        let path = entry.unwrap().path();
        if path.is_dir() {
            any_file_contains(&path, needle)
        } else {
            std::fs::read_to_string(&path).is_ok_and(|text| text.contains(needle))
        }
    })
}

#[test]
fn import_saves_the_account_into_the_adapter_root() {
    let src = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = write_source(
        src.path(),
        &format!(r#"{{"oauthAccount":{{"id":"{UUID}","emailAddress":"user@example.com"}}}}"#),
    );
    import_auth_from_file(&source, root.path()).unwrap();
    assert!(any_file_contains(root.path(), UUID));
    assert!(any_file_contains(root.path(), "user@example.com"));
}

#[test]
fn import_names_the_file_when_it_is_missing() {
    let src = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let err = import_auth_from_file(&src.path().join(".claude.json"), root.path())
        .expect_err("a missing source must fail");
    assert!(format!("{err:#}").contains("reading"), "{err:#}");
}

#[test]
fn import_names_the_file_when_it_is_not_json() {
    let src = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = write_source(src.path(), "{ not json");
    let err = import_auth_from_file(&source, root.path()).expect_err("bad JSON must fail");
    assert!(format!("{err:#}").contains("parsing"), "{err:#}");
}

#[test]
fn import_says_how_to_log_in_when_there_is_no_account() {
    let src = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = write_source(src.path(), r#"{"numStartups":3}"#);
    let err = import_auth_from_file(&source, root.path()).expect_err("no account must fail");
    let text = format!("{err:#}");
    assert!(text.contains("has no oauthAccount block"), "{text}");
    assert!(text.contains("llmenv login"), "{text}");
}

#[test]
fn import_rejects_an_account_with_an_empty_id() {
    let src = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = write_source(
        src.path(),
        r#"{"oauthAccount":{"id":"","emailAddress":"a@b.c"}}"#,
    );
    assert!(import_auth_from_file(&source, root.path()).is_err());
    assert!(!any_file_contains(root.path(), "a@b.c"));
}

#[test]
fn store_login_saves_the_account_from_the_login_dir() {
    let login = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    write_source(
        login.path(),
        &format!(r#"{{"oauthAccount":{{"id":"{UUID}","emailAddress":"user@example.com"}}}}"#),
    );
    super::store_login(root.path(), login.path(), None).unwrap();
    assert!(any_file_contains(root.path(), UUID));
}

#[test]
fn store_login_names_the_fix_when_the_login_left_no_account() {
    let login = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let err = super::store_login(root.path(), login.path(), None)
        .expect_err("a login with no account must fail");
    let text = format!("{err:#}");
    assert!(text.contains("no oauthAccount found"), "{text}");
    assert!(text.contains("claude auth login"), "{text}");
}

#[test]
fn store_login_writes_the_account_into_the_current_folder() {
    let login = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    write_source(
        login.path(),
        &format!(r#"{{"oauthAccount":{{"id":"{UUID}","emailAddress":"user@example.com"}}}}"#),
    );
    super::store_login(root.path(), login.path(), Some(folder.path())).unwrap();
    assert!(any_file_contains(folder.path(), UUID));
}
