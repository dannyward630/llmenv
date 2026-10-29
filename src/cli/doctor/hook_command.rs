//! Doctor check for the program name in llmenv's hook commands (#3049, #3081). The hook commands
//! name the running llmenv binary. The bare name is used only when that binary's path is not
//! UTF-8 or the OS cannot report it.

use crate::adapter::hook_program::program_name;

/// Print the hook-command check for the running binary.
pub(super) fn run_doctor_hook_command(use_color: bool) {
    let pass = super::super::doctor_pass(use_color);
    let warn = super::super::doctor_warning(use_color);
    match program_name(std::env::current_exe()).fallback_warning() {
        Some(message) => eprintln!("{warn} {message}"),
        None => eprintln!("{pass} Hook commands name llmenv by the path of the running binary"),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test fails loudly when its fixture cannot be set up"
)]
mod tests {
    use super::*;

    #[test]
    fn warns_with_the_fix_when_the_binary_path_is_unknown() {
        let warning = program_name(Err(std::io::Error::other("no path")))
            .fallback_warning()
            .expect("the bare name is warned about");
        assert!(warning.contains("Fix: run llmenv"), "{warning}");
    }

    #[test]
    fn no_warning_when_the_binary_path_is_known() {
        let exe = std::path::PathBuf::from("/opt/llmenv/bin/llmenv");
        assert_eq!(program_name(Ok(exe)).fallback_warning(), None);
    }
}
