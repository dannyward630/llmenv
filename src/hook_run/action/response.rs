//! Clean-up of an ICM recall response before it reaches the agent.

/// Strip advisor/no-memory lines from an ICM server recall response.
///
/// The ICM server appends `[ICM: N tool calls … Consider saving …]` and/or
/// `No memories found.` to recall responses. These lines are useful in an
/// interactive session but inject ~1KB of pure noise per user turn when piped
/// through the hook-runner's `emit_hook_context` (issue #692).
///
/// Only the two advisory line shapes are stripped — `No memories found.` (the
/// empty-store marker) and the `[ICM: N tool calls … Consider saving …]`
/// counter/nag — matched at the *start* of the trimmed line, case-insensitively.
///
/// Anchoring to the line prefix (not a bare substring scan of arbitrary prose)
/// is deliberate: a recalled memory whose body merely contains "consider
/// saving" or "no memories found" mid-sentence must pass through unchanged, or
/// it would be silently dropped from the injected context. Case-insensitive so
/// a wording/case variant from the server is still recognized (#978).
pub(crate) fn strip_advisory(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let lower = line.trim_start().to_ascii_lowercase();
            !lower.starts_with("no memories found") && !lower.starts_with("[icm:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ===== #692: ICM advisory line stripping =====

    #[test]
    fn strip_advisory_passes_normal_memory_text() {
        let text = "## Project Context\n- Key configuration\n- Recent changes";
        assert_eq!(strip_advisory(text), text);
    }

    #[test]
    fn strip_advisory_removes_no_memories_found() {
        let text = "No memories found.";
        assert!(strip_advisory(text).is_empty());
    }

    #[test]
    fn strip_advisory_removes_consider_saving() {
        let text = "[ICM: 42 tool calls since last store. Consider saving important context with icm_memory_store before it is lost.]";
        assert!(strip_advisory(text).is_empty());
    }

    #[test]
    fn strip_advisory_removes_both_advisory_lines() {
        let text = "No memories found.\n[ICM: 3 tool calls since last store. Consider saving important context with icm_memory_store before it is lost.]";
        assert!(strip_advisory(text).is_empty());
    }

    #[test]
    fn strip_advisory_preserves_real_content_alongside_advisory() {
        let text = "## Project Context\n- Key configuration\nNo memories found.\n[ICM: 3 tool calls. Consider saving.]\n- Recent changes";
        assert_eq!(
            strip_advisory(text),
            "## Project Context\n- Key configuration\n- Recent changes"
        );
    }

    // #978: the server's wording/case may drift from the exact literals. A
    // case/punctuation variant must still be stripped so an empty store injects
    // nothing rather than a "No memories found" block on every turn.
    #[test]
    fn strip_advisory_is_case_insensitive() {
        let text = "no memories found\n[icm: 5 tool calls since last store. consider saving important context.]";
        assert!(
            strip_advisory(text).is_empty(),
            "lowercase advisory variants must still be stripped"
        );
    }

    // A recalled memory whose body merely contains the advisory phrases
    // mid-sentence must survive — the filter anchors on the line prefix, not a
    // bare substring, so real content is never silently dropped.
    #[test]
    fn strip_advisory_preserves_prose_containing_advisory_phrases() {
        let text = "Consider saving the connection pool between requests\nThe runbook said no memories found in the cache tier";
        assert_eq!(strip_advisory(text), text);
    }

    proptest! {
        // Idempotent: re-stripping already-stripped text is a no-op.
        #[test]
        fn prop_strip_advisory_idempotent(s in ".*") {
            let once = strip_advisory(&s);
            prop_assert_eq!(strip_advisory(&once), once.clone());
        }

        // No advisory line survives: after stripping, no line starts with an
        // advisory marker (case-insensitively).
        #[test]
        fn prop_strip_advisory_leaves_no_advisory_line(s in ".*") {
            let out = strip_advisory(&s);
            for line in out.lines() {
                let lower = line.trim_start().to_ascii_lowercase();
                prop_assert!(!lower.starts_with("no memories found"));
                prop_assert!(!lower.starts_with("[icm:"));
            }
        }
    }
}
