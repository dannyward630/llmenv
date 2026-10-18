//! Tool failures that carry no memory-worthy signal (#2910).
//!
//! A typo (exit 127) or a parse error in the agent's own script matches memories from every
//! project. A recall for such a failure injects unrelated notes, so the recall is skipped.

/// Text that marks a generic failure. Matched case-insensitively.
const GENERIC_MARKERS: [&str; 5] = [
    "exit code 127",
    "command not found",
    "syntaxerror",
    "json.parse",
    "unexpected token",
];

/// Whether `error` is a generic failure, so a failure recall would inject unrelated memories.
pub(super) fn is_generic(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    GENERIC_MARKERS.iter().any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::is_generic;

    #[test]
    fn a_typo_and_a_script_parse_error_are_generic() {
        assert!(is_generic("Exit code 127\nzsh: command not found: gti"));
        assert!(is_generic(
            "SyntaxError: Unexpected token } in JSON at position 4"
        ));
    }

    #[test]
    fn a_failing_test_is_not_generic() {
        assert!(!is_generic(
            "Exit code 1\nthread 'parse' panicked at src/lib.rs:4"
        ));
        assert!(!is_generic(""));
    }
}
