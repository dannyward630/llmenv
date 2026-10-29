//! The command line that runs llmenv's built-in hooks, rendered into each engine's hook table
//! (#2956).

use std::ffi::OsStr;
use std::path::PathBuf;

use llmenv_util::shell::shell_word;

/// The command line that starts llmenv with `args`, such as `hook-run --engine crush`.
///
/// The program is the absolute path of llmenv on `PATH`, shell-quoted. A bare name would let the
/// engine search its working folder, where a cloned repository could hold its own `llmenv`.
/// When no absolute `PATH` folder holds llmenv, or its path is not UTF-8, the bare name is used
/// and a warning is logged.
pub(crate) fn hook_command(args: &str) -> String {
    hook_command_in(std::env::var_os("PATH").as_deref(), args)
}

/// How hook commands name the llmenv program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProgramName {
    /// The shell-quoted absolute path of llmenv on `PATH`.
    Absolute(String),
    /// The bare name `llmenv`. Holds the path found on `PATH` when it is not UTF-8, and `None`
    /// when no absolute `PATH` folder holds llmenv.
    Bare(Option<PathBuf>),
}

impl ProgramName {
    /// The program text that the hook command lines start with.
    fn word(&self) -> &str {
        match self {
            Self::Absolute(word) => word,
            Self::Bare(_) => "llmenv",
        }
    }

    /// The message that names the bare-name fallback and its fix, or `None` for an absolute path.
    pub(crate) fn fallback_warning(&self) -> Option<String> {
        match self {
            Self::Absolute(_) => None,
            // A lossy conversion would name a file that does not exist, so the bare name is used.
            Self::Bare(Some(path)) => Some(format!(
                "llmenv is at {}, which is not valid UTF-8, so its hook commands use the bare \
                 name `llmenv`. Fix: install llmenv in a UTF-8 folder on PATH.",
                path.display()
            )),
            Self::Bare(None) => Some(
                "llmenv is not in an absolute PATH folder, so its hook commands use the bare \
                 name `llmenv`, which a relative PATH entry can shadow. Fix: install llmenv in \
                 an absolute PATH folder, such as ~/.cargo/bin."
                    .to_string(),
            ),
        }
    }
}

/// Pick how hook commands name llmenv, from the `PATH` value in `path_var`.
pub(crate) fn program_name(path_var: Option<&OsStr>) -> ProgramName {
    match llmenv_paths::program::find_on_path("llmenv", path_var) {
        Some(path) if path.to_str().is_some() => ProgramName::Absolute(shell_word(&path)),
        found => ProgramName::Bare(found),
    }
}

fn hook_command_in(path_var: Option<&OsStr>, args: &str) -> String {
    let program = program_name(path_var);
    if let Some(warning) = program.fallback_warning() {
        tracing::warn!("{warning}");
    }
    format!("{} {args}", program.word())
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

    #[test]
    fn bare_fallback_without_path_names_the_absolute_folder_fix() {
        let program = program_name(None);
        assert_eq!(program, ProgramName::Bare(None));
        let warning = program
            .fallback_warning()
            .expect("the bare name has a warning");
        assert!(warning.contains("absolute PATH folder"), "{warning}");
        assert!(warning.contains("Fix: install llmenv"), "{warning}");
    }

    #[cfg(unix)]
    #[test]
    fn absolute_program_has_no_fallback_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        make_executable(&dir.path().join("llmenv"));
        let path_var = OsString::from(dir.path());

        let program = program_name(Some(&path_var));

        assert!(matches!(program, ProgramName::Absolute(_)), "{program:?}");
        assert_eq!(program.fallback_warning(), None);
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
