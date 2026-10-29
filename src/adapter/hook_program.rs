//! The command line that runs llmenv's built-in hooks, rendered into each engine's hook table
//! (#2956, #3081).

use std::path::PathBuf;

use crate::util::shell_word;

/// The command line that starts llmenv with `args`, such as `hook-run --engine crush`.
///
/// The program is the running llmenv binary, shell-quoted. `PATH` is not searched, so a
/// project folder on `PATH` cannot be written into an engine config. When the binary's path is
/// not UTF-8 or the OS cannot report it, the bare name is used and a warning is logged.
pub(crate) fn hook_command(args: &str) -> String {
    hook_command_in(std::env::current_exe(), args)
}

/// How hook commands name the llmenv program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProgramName {
    /// The shell-quoted path of the running llmenv binary.
    Absolute(String),
    /// The bare name `llmenv`, with the reason the binary's path cannot be used.
    Bare(String),
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
            Self::Bare(reason) => Some(format!(
                "{reason}, so its hook commands use the bare name `llmenv`. Fix: run llmenv \
                 from a binary whose path is UTF-8 and reported by the OS."
            )),
        }
    }
}

/// Pick how hook commands name llmenv, from the running binary's path in `exe`.
pub(crate) fn program_name(exe: std::io::Result<PathBuf>) -> ProgramName {
    match exe {
        Ok(path) if path.to_str().is_some() => ProgramName::Absolute(shell_word(&path)),
        // A lossy conversion would name a file that does not exist, so the bare name is used.
        Ok(path) => ProgramName::Bare(format!(
            "llmenv is at {}, which is not valid UTF-8",
            path.display()
        )),
        Err(e) => ProgramName::Bare(format!("the path of the running llmenv is unknown ({e})")),
    }
}

fn hook_command_in(exe: std::io::Result<PathBuf>, args: &str) -> String {
    let program = program_name(exe);
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

    #[test]
    fn renders_the_running_binary_path_not_a_path_search() {
        let exe = PathBuf::from("/opt/llmenv/bin/llmenv");
        let command = hook_command_in(Ok(exe), "hook-run --engine crush pre_tool_use");
        assert_eq!(
            command,
            "/opt/llmenv/bin/llmenv hook-run --engine crush pre_tool_use"
        );
    }

    #[test]
    fn absolute_program_has_no_fallback_warning() {
        let program = program_name(Ok(PathBuf::from("/opt/llmenv/bin/llmenv")));
        assert!(matches!(program, ProgramName::Absolute(_)), "{program:?}");
        assert_eq!(program.fallback_warning(), None);
    }

    #[test]
    fn an_unknown_binary_path_falls_back_to_the_bare_name_with_a_warning() {
        let program = program_name(Err(std::io::Error::other("no such file")));
        let warning = program
            .fallback_warning()
            .expect("the bare name has a warning");
        assert!(warning.contains("no such file"), "{warning}");
        assert_eq!(
            hook_command_in(Err(std::io::Error::other("gone")), "throttle"),
            "llmenv throttle"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_binary_path_falls_back_to_the_bare_name_with_a_warning() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let exe = PathBuf::from(OsString::from_vec(vec![b'/', 0xff, b'/', b'l']));
        let program = program_name(Ok(exe));
        let warning = program
            .fallback_warning()
            .expect("the bare name has a warning");
        assert!(warning.contains("not valid UTF-8"), "{warning}");
    }

    proptest! {
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
