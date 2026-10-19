//! Find a program on `PATH` without a shell. The lookup lives in `llmenv-paths`, so the
//! `llmenv-git` crate uses it too (#2948).

pub(crate) use llmenv_paths::program::{find_on_path, relative_entries, resolve_program};

#[cfg(all(test, unix))]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn a_folder_that_cannot_be_read_is_logged() {
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        let tool = locked.join("hidden");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let path_var = std::env::join_paths([&locked]).unwrap();
        let logs = crate::test_log_capture::capture_logs(|| {
            assert_eq!(find_on_path("hidden", Some(&path_var)), None);
        });
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(logs.contains("cannot check"), "{logs}");
    }
}
