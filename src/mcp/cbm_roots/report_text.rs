//! The doctor and session-start text for a roots report (#2916, #2928).

use std::path::{Path, PathBuf};

use super::RootsReport;

/// The text that tells the user what `report` found. `warn` is true when a declared root is
/// missing. The user runs the commands in the text; llmenv never does (#2916).
pub(crate) fn describe(report: &RootsReport) -> (bool, String) {
    // An empty record leaves the server unconfined, so no root is missing or undeclared (#2928).
    if report.listed.is_empty() {
        return (
            false,
            format!(
                "codebase-memory: no allowed roots are recorded, so indexing is unconfined \
                 (cache {}).",
                report.cache_dir.display()
            ),
        );
    }
    let list = |roots: &[PathBuf]| {
        roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let undeclared = if report.undeclared.is_empty() {
        String::new()
    } else {
        format!(
            " The server also holds roots that the config does not declare: {}. Earlier llmenv \
             versions recorded them. Ask the user to clear them: to return to unconfined \
             indexing, the user deletes {}; to keep some roots, the user edits that file. Do not \
             delete or edit it yourself.",
            list(&report.undeclared),
            report.cache_dir.join("allowed_roots").display()
        )
    };
    if report.missing.is_empty() {
        return (
            false,
            format!(
                "codebase-memory: allowed roots {} (cache {}).{undeclared}",
                list(&report.listed),
                report.cache_dir.display()
            ),
        );
    }
    let missing: Vec<String> = report
        .missing
        .iter()
        .map(|(root, why)| format!("{} ({why})", root.display()))
        .collect();
    let commands: Vec<String> = report
        .missing
        .iter()
        .map(|(root, _)| allow_root_command(&report.cache_dir, root))
        .collect();
    (
        true,
        format!(
            "codebase-memory cannot index {}. Ask the user to run: {}. llmenv never runs \
             allow-root itself.{undeclared}",
            missing.join(", "),
            commands.join("; ")
        ),
    )
}

/// The command that the user runs to record `root` with the server of `cache_dir`.
fn allow_root_command(cache_dir: &Path, root: &Path) -> String {
    format!(
        "CBM_CACHE_DIR={} codebase-memory-mcp allow-root {}",
        crate::util::shell_word(cache_dir),
        crate::util::shell_word(root)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_description_lists_the_roots_or_the_missing_ones_with_the_fix() {
        let ok = RootsReport {
            listed: vec!["/a".into(), "/b".into()],
            missing: vec![],
            undeclared: vec![],
            cache_dir: "/cache".into(),
        };
        assert_eq!(
            describe(&ok),
            (
                false,
                "codebase-memory: allowed roots /a, /b (cache /cache).".to_string()
            )
        );
        let bad = RootsReport {
            missing: vec![("/c".into(), "refused".to_string())],
            ..ok
        };
        let (warn, text) = describe(&bad);
        assert!(warn);
        assert!(text.contains("/c (refused)"), "{text}");
        assert!(
            text.contains("CBM_CACHE_DIR=/cache codebase-memory-mcp allow-root /c"),
            "{text}"
        );
        assert!(text.contains("llmenv never runs"), "{text}");
    }

    #[test]
    fn the_description_says_unconfined_when_no_root_is_recorded() {
        let none = RootsReport {
            listed: vec![],
            missing: vec![],
            undeclared: vec![],
            cache_dir: "/cache".into(),
        };
        assert_eq!(
            describe(&none),
            (
                false,
                "codebase-memory: no allowed roots are recorded, so indexing is unconfined \
                 (cache /cache)."
                    .to_string()
            )
        );
    }

    #[test]
    fn the_description_names_the_undeclared_roots_and_the_file_to_clear() {
        let report = RootsReport {
            listed: vec!["/git".into()],
            missing: vec![],
            undeclared: vec!["/git".into()],
            cache_dir: "/cache".into(),
        };
        let (warn, text) = describe(&report);
        assert!(!warn);
        assert!(text.contains("/git"), "{text}");
        assert!(text.contains("/cache/allowed_roots"), "{text}");
    }
}
