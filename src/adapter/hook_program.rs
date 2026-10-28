//! The command line that runs llmenv's built-in hooks, rendered into each engine's hook table
//! (#2956).

use std::ffi::OsStr;

use crate::util::shell_word;

/// The command line that starts llmenv with `args`, such as `hook-run --engine crush`.
///
/// The program is the absolute path of llmenv on `PATH`, shell-quoted. A bare name would let the
/// engine search its working folder, where a cloned repository could hold its own `llmenv`.
/// When no absolute `PATH` folder holds llmenv, or its path is not UTF-8, the bare name is used
/// and a warning is logged.
pub(crate) fn hook_command(args: &str) -> String {
    hook_command_in(std::env::var_os("PATH").as_deref(), args)
}

fn hook_command_in(path_var: Option<&OsStr>, args: &str) -> String {
    let program = match llmenv_paths::program::find_on_path("llmenv", path_var) {
        Some(path) if path.to_str().is_some() => shell_word(&path),
        Some(path) => {
            // A lossy conversion would name a file that does not exist, so the bare name is used.
            tracing::warn!(
                "llmenv is at {}, which is not valid UTF-8, so its hook commands use the bare \
                 name `llmenv`. Fix: install llmenv in a UTF-8 folder on PATH.",
                path.display()
            );
            "llmenv".to_string()
        }
        None => {
            tracing::warn!(
                "llmenv is not in an absolute PATH folder, so its hook commands use the bare \
                 name `llmenv`, which a relative PATH entry can shadow. Fix: install llmenv in \
                 an absolute PATH folder, such as ~/.cargo/bin."
            );
            "llmenv".to_string()
        }
    };
    format!("{program} {args}")
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test fails loudly when its fixture cannot be set up"
)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::ffi::OsString;

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, "#!/bin/sh\n").expect("write fake llmenv");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake llmenv");
    }

    #[cfg(unix)]
    #[test]
    fn uses_absolute_path_when_llmenv_is_on_an_absolute_path_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        make_executable(&dir.path().join("llmenv"));
        let path_var = OsString::from(dir.path());

        let command = hook_command_in(Some(&path_var), "hook-run --engine crush pre_tool_use");

        let expected = format!(
            "{} hook-run --engine crush pre_tool_use",
            shell_word(&dir.path().join("llmenv"))
        );
        assert_eq!(command, expected);
    }

    #[cfg(unix)]
    #[test]
    fn ignores_a_planted_llmenv_in_a_relative_path_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        make_executable(&dir.path().join("llmenv"));
        let path_var = OsString::from(".");

        let command = hook_command_in(Some(&path_var), "throttle");

        assert_eq!(command, "llmenv throttle");
    }

    #[test]
    fn falls_back_to_bare_name_when_path_is_unset() {
        assert_eq!(hook_command_in(None, "throttle"), "llmenv throttle");
    }

    proptest! {
        /// A PATH made only of relative folders falls back to the bare name, whatever it holds.
        #[test]
        fn relative_only_path_always_falls_back_to_bare_name(
            entries in proptest::collection::vec("[a-zA-Z0-9._/-]{0,12}", 0..6),
        ) {
            let relative: Vec<String> = entries.iter().map(|e| format!("rel/{e}")).collect();
            let path_var = OsString::from(relative.join(":"));
            prop_assert_eq!(hook_command_in(Some(&path_var), "throttle"), "llmenv throttle");
        }

        /// The quoted program is one shell word: a shell that reads it back gets the same text.
        #[cfg(unix)]
        #[test]
        fn quoted_program_round_trips_through_a_shell(text in "[^\u{0}]{1,40}") {
            let word = shell_word(std::path::Path::new(&text));
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("printf '%s' {word}"))
                .output()
                .expect("run /bin/sh");
            prop_assert_eq!(String::from_utf8_lossy(&out.stdout).into_owned(), text);
        }
    }
}
