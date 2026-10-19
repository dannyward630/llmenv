use std::os::unix::fs::symlink;

use super::*;

const CAP: u64 = 1024;

#[test]
fn a_regular_marker_reads_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join(".llmenv.yaml");
    std::fs::write(&marker, "id: a\n").unwrap();
    assert_eq!(
        read_marker(&marker, CAP).unwrap().as_deref(),
        Some("id: a\n")
    );
    assert_eq!(
        read_marker(&dir.path().join("none.yaml"), CAP).unwrap(),
        None
    );
}

#[test]
fn a_link_that_stays_inside_the_folder_is_followed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("dotfiles/sub")).unwrap();
    std::fs::write(dir.path().join("dotfiles/sub/llmenv.yaml"), "id: dot\n").unwrap();
    symlink("sub", dir.path().join("dotfiles/link")).unwrap();
    let marker = dir.path().join(".llmenv.yaml");
    symlink("dotfiles/link/../sub/llmenv.yaml", &marker).unwrap();
    assert_eq!(
        read_marker(&marker, CAP).unwrap().as_deref(),
        Some("id: dot\n")
    );
}

#[test]
fn an_absolute_link_inside_the_folder_is_followed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("real.yaml"), "id: abs\n").unwrap();
    let marker = root.join(".llmenv.yaml");
    symlink(root.join("real.yaml"), &marker).unwrap();
    assert_eq!(
        read_marker(&marker, CAP).unwrap().as_deref(),
        Some("id: abs\n")
    );
}

#[test]
fn a_link_out_of_the_folder_is_refused_and_named() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("x.yaml"), "description: secret\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join(".llmenv.yaml");
    for target in [
        outside.path().join("x.yaml"),
        PathBuf::from("../escape.yaml"),
        PathBuf::from("/net/host/x"),
    ] {
        let _ = std::fs::remove_file(&marker);
        symlink(&target, &marker).unwrap();
        let err = read_marker(&marker, CAP).unwrap_err().to_string();
        assert!(
            err.contains(".llmenv.yaml") && err.contains("leaves the folder"),
            "{err}"
        );
    }
}

#[test]
fn a_link_through_a_folder_link_out_of_the_folder_is_refused() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("x.yaml"), "id: out\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    symlink(outside.path(), dir.path().join("sub")).unwrap();
    let marker = dir.path().join(".llmenv.yaml");
    symlink("sub/x.yaml", &marker).unwrap();
    let err = read_marker(&marker, CAP).unwrap_err().to_string();
    assert!(err.contains("leaves the folder"), "{err}");
}

#[test]
fn a_link_loop_and_a_dangling_link_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join(".llmenv.yaml");
    symlink(".llmenv.yaml", &marker).unwrap();
    let err = read_marker(&marker, CAP).unwrap_err().to_string();
    assert!(err.contains("too many links"), "{err}");
    std::fs::remove_file(&marker).unwrap();
    symlink("missing.yaml", &marker).unwrap();
    let err = read_marker(&marker, CAP).unwrap_err().to_string();
    assert!(err.contains("does not exist"), "{err}");
    assert!(err.contains("missing.yaml"), "names the target: {err}");
}
