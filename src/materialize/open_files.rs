//! The paths that processes hold open, as reported by `lsof`.
//!
//! The `state/tmp` sweep keeps an entry that a live process still uses. This module builds the
//! list of those paths for the sweep to check against.

use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use anyhow::Context;

/// Paths that a process holds open or uses as its working directory.
///
/// # Errors
/// Fails when `lsof` cannot run or does not exit 0. A sweep without a complete list could
/// remove an entry that a live process uses, so the sweep stops instead.
#[cfg(unix)]
pub fn open_paths() -> anyhow::Result<Vec<PathBuf>> {
    lsof_paths("lsof")
}

/// The sweep needs lsof to find the entries in use, and lsof is a unix tool.
///
/// # Errors
/// Always fails, because the in-use list cannot be built on this platform.
#[cfg(not(unix))]
pub fn open_paths() -> anyhow::Result<Vec<PathBuf>> {
    anyhow::bail!("the state/tmp sweep needs lsof, which runs on unix only; the sweep stopped")
}

/// Run `program -F n` and read the paths it names. `program` is a parameter so a test can
/// name a binary that fails.
#[cfg(unix)]
fn lsof_paths(program: &str) -> anyhow::Result<Vec<PathBuf>> {
    let output = Command::new(program)
        .args(["-F", "n"])
        .output()
        .with_context(|| {
            format!(
                "running `{program}` to find entries in use; the state/tmp sweep stopped. \
                 Fix: install lsof, then run `llmenv doctor --gc` again"
            )
        })?;
    // lsof exits 1 on any error, and its list is then incomplete. Only exit 0 counts.
    if output.status.code() != Some(0) {
        anyhow::bail!(
            "`{program}` exited with {}; the state/tmp sweep stopped. Fix: check that `{program}` runs",
            output.status
        );
    }
    // lsof reports partial results on stderr, for example for a mount it cannot stat. Those
    // results are routine on some hosts, so they are logged, not fatal.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        tracing::warn!(
            stderr = %stderr.trim(),
            "lsof reported a problem; the in-use list may be incomplete"
        );
    }
    Ok(parse_lsof_names(&output.stdout))
}

/// The name fields of `lsof -F n` output that are absolute paths. Socket and pipe names do not
/// start with `/`, so they drop out. Names are kept as bytes, so a non-UTF-8 name still matches.
#[cfg(unix)]
fn parse_lsof_names(output: &[u8]) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    output
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.strip_prefix(b"n"))
        .filter(|name| name.starts_with(b"/"))
        .map(|name| PathBuf::from(std::ffi::OsStr::from_bytes(name)))
        .collect()
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::{lsof_paths, parse_lsof_names};

    #[test]
    fn parse_keeps_only_the_absolute_path_names() {
        let out = b"p123\nfcwd\nn/work/a\nnTCP *:8080\np9\nn/work/b (deleted)\n";
        let paths = parse_lsof_names(out);
        assert_eq!(
            paths,
            vec![
                std::path::PathBuf::from("/work/a"),
                std::path::PathBuf::from("/work/b (deleted)"),
            ]
        );
    }

    #[test]
    fn parse_keeps_a_non_utf8_name_byte_for_byte() {
        use std::os::unix::ffi::OsStrExt;
        let paths = parse_lsof_names(b"n/tmp/\xff\n");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].as_os_str().as_bytes(), b"/tmp/\xff");
    }

    #[test]
    fn parse_yields_one_path_per_absolute_name_line() {
        use proptest::prelude::*;
        proptest!(|(lines in proptest::collection::vec("[pfn/ a-z]{0,10}", 0..12))| {
            let text = lines.join("\n");
            let expected: Vec<String> = lines
                .iter()
                .filter(|line| line.starts_with("n/"))
                .map(|line| line[1..].to_owned())
                .collect();
            let got: Vec<String> = parse_lsof_names(text.as_bytes())
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            prop_assert_eq!(got, expected);
        });
    }

    #[test]
    fn a_missing_lsof_is_an_error_that_names_the_fix() {
        let err = lsof_paths("llmenv-no-such-lsof-binary").unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("Fix:"), "no fix named: {text}");
        assert!(
            text.contains("state/tmp sweep stopped"),
            "no effect named: {text}"
        );
    }

    #[test]
    fn an_lsof_exit_other_than_zero_is_an_error_even_with_empty_output() {
        // `false` exits 1 and prints nothing. An exit 1 is a failure, not an empty list.
        let err = lsof_paths("false").unwrap_err();
        assert!(format!("{err:#}").contains("exited with"), "{err:#}");
    }
}
