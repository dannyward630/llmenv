//! The Claude Code version, for the hooks (#2898).
//!
//! Claude Code does not export its version to hook processes. llmenv reads it once per
//! materialize and passes it as `CLAUDE_CODE_VERSION`, so session metadata can record it.

use std::process::{Command, Stdio};
use std::time::Duration;

use crate::process::run_with_timeout;

/// How long `claude --version` may take. The binary prints one line and exits.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// The `CLAUDE_CODE_VERSION` variable, or no variable when the version cannot be read. The hooks
/// then record an empty version, as they did before this change.
pub(crate) fn env_vars() -> Vec<(String, String)> {
    // Export runs in the folder of each shell prompt, so a bare `claude` could run a copy that a
    // cloned repo holds in a relative PATH folder (#2932).
    let claude = match crate::path_lookup::resolve_program("claude") {
        Ok(path) => path,
        Err(e) => {
            tracing::error!("CLAUDE_CODE_VERSION not set: {e:#}");
            return Vec::new();
        }
    };
    let mut cmd = Command::new(claude);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match run_with_timeout(cmd, VERSION_TIMEOUT) {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            match parse_version(&text) {
                Some(version) => vec![("CLAUDE_CODE_VERSION".into(), version)],
                None => {
                    tracing::error!("claude --version printed no version: {}", text.trim());
                    Vec::new()
                }
            }
        }
        Ok(out) => {
            tracing::error!("claude --version exited with {}", out.status);
            Vec::new()
        }
        Err(e) => {
            tracing::error!("claude --version failed: {e:#}");
            Vec::new()
        }
    }
}

/// The version number at the start of `claude --version` output, such as `2.1.251` from
/// `2.1.251 (Claude Code)`. `None` when the first word is not a dotted number.
fn parse_version(text: &str) -> Option<String> {
    let first = text.split_whitespace().next()?;
    let numeric = first.starts_with(|c: char| c.is_ascii_digit())
        && first
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    numeric.then(|| first.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn the_version_is_the_first_word_of_the_output() {
        assert_eq!(
            parse_version("2.1.251 (Claude Code)\n").as_deref(),
            Some("2.1.251")
        );
    }

    #[test]
    fn output_without_a_dotted_number_has_no_version() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("error: unknown flag"), None);
        assert_eq!(parse_version("v2.1.251"), None);
        assert_eq!(
            parse_version("2.1.251-beta"),
            Some("2.1.251-beta".to_string())
        );
    }
}
