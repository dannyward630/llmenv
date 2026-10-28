//! Doctor check for the program name in llmenv's hook commands (#3049). With no absolute `PATH`
//! folder that holds llmenv, the hook commands use the bare name, and a relative `PATH` entry can
//! shadow that name.

use std::ffi::OsStr;

use crate::adapter::hook_program::program_name;

/// The warning for `path_var`, or `None` when the hook commands name llmenv by an absolute path.
fn hook_command_warning(path_var: Option<&OsStr>) -> Option<String> {
    program_name(path_var).fallback_warning()
}

/// Print the hook-command check for the current `PATH`.
pub(super) fn run_doctor_hook_command(use_color: bool) {
    let pass = super::super::doctor_pass(use_color);
    let warn = super::super::doctor_warning(use_color);
    match hook_command_warning(std::env::var_os("PATH").as_deref()) {
        Some(message) => eprintln!("{warn} {message}"),
        None => eprintln!("{pass} Hook commands name llmenv by an absolute PATH folder"),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test fails loudly when its fixture cannot be set up"
)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn warns_with_the_fix_when_path_is_unset() {
        let warning = hook_command_warning(None).expect("the bare name is warned about");
        assert!(warning.contains("Fix: install llmenv"), "{warning}");
    }

    #[cfg(unix)]
    #[test]
    fn no_warning_when_llmenv_is_on_an_absolute_path_folder() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let binary = dir.path().join("llmenv");
        std::fs::write(&binary, "#!/bin/sh\n").expect("write fake llmenv");
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake llmenv");
        let path_var = OsString::from(dir.path());

        assert_eq!(hook_command_warning(Some(&path_var)), None);
    }

    #[cfg(unix)]
    #[test]
    fn warns_when_llmenv_is_only_in_a_relative_path_folder() {
        let path_var = OsString::from(".");
        assert!(hook_command_warning(Some(&path_var)).is_some());
    }
}
