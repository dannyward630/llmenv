//! Find a program on `PATH` without a shell (#2932, #2948).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The first executable file named `name` in the absolute folders of `path_var`.
///
/// A relative folder such as `.` names a different place in each working directory, so a file
/// planted there could stand in for the real program. Such a folder is skipped (#2932). A name
/// that holds a path separator is not looked up, so a caller cannot point it at an arbitrary path.
pub fn find_on_path(name: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    if name.is_empty() || name.contains(std::path::is_separator) {
        return None;
    }
    std::env::split_paths(path_var?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// The program to spawn for `name`. A name that holds a path separator is a path that the config
/// gives, and it is used as it is. A bare name is looked up in the absolute folders of `PATH`,
/// because a spawn by bare name searches the relative folders too (#2932).
///
/// # Errors
/// The name is not in any absolute folder of `PATH`.
pub fn resolve_program(name: &str) -> anyhow::Result<PathBuf> {
    if name.contains(std::path::is_separator) {
        return Ok(PathBuf::from(name));
    }
    find_on_path(name, std::env::var_os("PATH").as_deref()).ok_or_else(|| {
        anyhow::anyhow!(
            "`{name}` is not in any absolute PATH folder (relative folders such as `.` are \
             skipped). Fix: install `{name}`, or add its folder to PATH as an absolute path."
        )
    })
}

/// The relative folders of `path_var`, which the lookups skip. Doctor names them, so a program
/// that only such a folder holds does not read as plainly missing.
pub fn relative_entries(path_var: Option<&OsStr>) -> Vec<PathBuf> {
    path_var
        .map(|var| {
            std::env::split_paths(var)
                .filter(|dir| !dir.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the current user can run the file at `path`. `access(X_OK)` checks the owner, group,
/// and other bits against this user, which a mode-bit test does not.
/// A folder that cannot be read is logged, so a program in it does not read as plainly missing.
fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && can_run(path),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            false
        }
        Err(e) => {
            tracing::error!("cannot check {} for a program on PATH: {e}", path.display());
            false
        }
    }
}

#[cfg(unix)]
fn can_run(path: &Path) -> bool {
    rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

#[cfg(not(unix))]
fn can_run(_path: &Path) -> bool {
    true
}

#[cfg(all(test, unix))]
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
    fn a_program_path_is_used_as_it_is_and_a_missing_name_says_why() {
        assert_eq!(
            resolve_program("/opt/x/tool").unwrap(),
            PathBuf::from("/opt/x/tool")
        );
        let err = resolve_program("llmenv-no-such-program-2932").unwrap_err();
        assert!(
            format!("{err:#}").contains("absolute PATH folder"),
            "{err:#}"
        );
    }

    #[test]
    fn the_relative_entries_are_listed() {
        let path_var = std::env::join_paths(["/usr/bin", ".", "node_modules/.bin"]).unwrap();
        assert_eq!(
            relative_entries(Some(&path_var)),
            [PathBuf::from("."), PathBuf::from("node_modules/.bin")]
        );
        assert!(relative_entries(None).is_empty());
    }

    proptest::proptest! {
        #[test]
        fn a_found_program_sits_in_an_absolute_folder_of_the_list(
            entries in proptest::collection::vec("(/usr/bin|/bin|\\.|bin|[a-z]{1,4}/[a-z]{1,4})", 0..6),
            name in "(sh|env|ls|[a-z]{1,8})",
        ) {
            let path_var = std::env::join_paths(&entries).unwrap();
            if let Some(found) = find_on_path(&name, Some(&path_var)) {
                let parent = found.parent().unwrap();
                proptest::prop_assert!(parent.is_absolute());
                proptest::prop_assert!(entries.iter().any(|e| Path::new(e) == parent));
            }
            proptest::prop_assert_eq!(find_on_path(&format!("a/{name}"), Some(&path_var)), None);
        }
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
