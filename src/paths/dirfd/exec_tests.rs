use std::ffi::OsStr;
use std::os::fd::AsFd;

use super::{is_executable_at, open_dir_nofollow};

/// Any one of the three execute bits makes a file executable; a missing entry is not.
#[test]
fn any_execute_bit_makes_an_entry_executable() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let dir = open_dir_nofollow(tmp.path()).unwrap();
    for (mode, expected) in [(0o700, true), (0o610, true), (0o601, true), (0o644, false)] {
        let name = format!("f{mode:o}");
        let path = tmp.path().join(&name);
        std::fs::write(&path, "x").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            is_executable_at(dir.as_fd(), OsStr::new(&name)).unwrap(),
            expected,
            "{mode:o}"
        );
    }
    assert!(!is_executable_at(dir.as_fd(), OsStr::new("missing")).unwrap());
}
