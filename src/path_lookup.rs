//! Find a program on `PATH` without a shell.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The first executable file named `name` in the absolute folders of `path_var`.
///
/// A relative folder such as `.` names a different place in each working directory, so a file
/// planted there could stand in for the real program. Such a folder is skipped (#2932). A name
/// that holds a path separator is not looked up, so a caller cannot point it at an arbitrary path.
pub(crate) fn find_on_path(name: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    if name.is_empty() || name.contains(std::path::is_separator) {
        return None;
    }
    std::env::split_paths(path_var?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// Whether the current user can run the file at `path`. `access(X_OK)` checks the owner, group,
/// and other bits against this user, which a mode-bit test does not.
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
        && rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn tool(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn only_a_runnable_file_counts() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_executable(&tool(dir.path(), "tool", 0o755)));
        assert!(!is_executable(&tool(dir.path(), "data", 0o644)));
        assert!(!is_executable(&dir.path().join("missing")));
        assert!(!is_executable(dir.path()), "a folder is not a program");
    }

    #[test]
    fn a_relative_path_folder_is_skipped() {
        // A folder under the working directory, so its relative name finds the file.
        let dir = tempfile::tempdir_in(".").unwrap();
        tool(dir.path(), "planted", 0o755);
        let relative = Path::new(".").join(dir.path().file_name().unwrap());
        assert!(
            is_executable(&relative.join("planted")),
            "the relative name resolves"
        );
        let path_var = std::env::join_paths([&relative]).unwrap();
        assert_eq!(find_on_path("planted", Some(&path_var)), None);
        let absolute = std::fs::canonicalize(dir.path()).unwrap();
        let path_var = std::env::join_paths([relative, absolute.clone()]).unwrap();
        assert_eq!(
            find_on_path("planted", Some(&path_var)),
            Some(absolute.join("planted"))
        );
    }
}
