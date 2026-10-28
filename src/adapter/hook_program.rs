//! The command line that runs llmenv's built-in hooks, rendered into each engine's hook table
//! (#2956).

use std::ffi::OsStr;

/// The command line that starts llmenv with `args`, such as `hook-run --engine crush`.
///
/// The program is the absolute path of llmenv on `PATH`, shell-quoted. A bare name would let the
/// engine search its working folder, where a cloned repository could hold its own `llmenv`.
/// When no absolute `PATH` folder holds llmenv, the bare name is used and a warning is logged.
pub(crate) fn hook_command(args: &str) -> String {
    hook_command_in(std::env::var_os("PATH").as_deref(), args)
}

fn hook_command_in(path_var: Option<&OsStr>, args: &str) -> String {
    let program = match llmenv_paths::program::find_on_path("llmenv", path_var) {
        Some(path) => shell_quote(&path.to_string_lossy()),
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

/// Quote `text` for a POSIX shell. A plain path is returned as it is.
fn shell_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+@:".contains(c));
    if plain {
        return text.to_string();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test fails loudly when its fixture cannot be set up"
)]
mod tests {
    use super::*;
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
            dir.path().join("llmenv").display()
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
    fn shell_quote_leaves_plain_paths_alone() {
        assert_eq!(
            shell_quote("/usr/local/bin/llmenv"),
            "/usr/local/bin/llmenv"
        );
    }

    #[test]
    fn shell_quote_wraps_spaces_and_escapes_single_quotes() {
        assert_eq!(
            shell_quote("/Users/a b/it's/llmenv"),
            r"'/Users/a b/it'\''s/llmenv'"
        );
    }

    #[test]
    fn shell_quote_wraps_empty_text() {
        assert_eq!(shell_quote(""), "''");
    }
}
