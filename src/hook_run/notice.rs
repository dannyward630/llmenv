//! Agent notices written to stdout, where the engine reads hook context (#2658, #2736).

/// Write a notice with no memory text to stdout, where the engine reads hook context (#2658).
pub(super) fn write_notice_only(
    adapter: &dyn crate::adapter::AgentAdapter,
    hook_event_name: &str,
    event: &str,
    notice: &str,
) {
    if hook_event_name.is_empty() {
        // The engine may reject an empty name, so the notice is also logged here, not left to the
        // engine's acceptance (#2748).
        tracing::error!("hook event '{event}' has no engine name; the notice may be rejected");
    }
    let out = adapter.emit_hook_context(hook_event_name, "", notice);
    log_notice_without_channel(&out, Some(notice), event);
    if let Some(e) = write_hook_line(&mut std::io::stdout(), &out) {
        eprintln!("llmenv: failed to write hook output: {e}");
    }
}

/// An engine that takes no hook context, such as the SessionEnd path, gets empty output, so a
/// notice in it reaches no one. The error log is the only place left for it (#2736).
pub(super) fn log_notice_without_channel(out: &str, notice: Option<&str>, event: &str) {
    if let Some(notice) = notice
        && out.is_empty()
    {
        tracing::error!(
            "hook-run {event} has no channel to the agent, so the notice is logged only: {notice}"
        );
    }
}

/// The notice for a session whose ICM transcript could not start. The map cannot save the
/// link, so the transcript is not started (#2658).
pub(super) fn transcript_unlinked_notice(error: &anyhow::Error) -> String {
    format!(
        "llmenv: no ICM transcript started for this session: {error:#}. \
         Fix: run `llmenv doctor` to check the state directory."
    )
}

/// Writes one hook output line to `w`. Returns the error to report, or `None` when there is no
/// output or the reader went away. A closed pipe is normal for a hook, so it is not an error.
pub(super) fn write_hook_line(w: &mut impl std::io::Write, out: &str) -> Option<std::io::Error> {
    if out.is_empty() {
        return None;
    }
    match writeln!(w, "{out}") {
        Ok(()) => None,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => None,
        Err(e) => Some(e),
    }
}

/// A notice that names the problem and the fix, in the form every agent notice uses.
pub(super) fn problem_notice(problem: &str, fix: &str) -> String {
    format!("llmenv: {problem}. Fix: {fix}")
}

#[cfg(test)]
mod tests {
    use super::problem_notice;

    #[test]
    fn a_problem_notice_names_the_problem_then_the_fix() {
        assert_eq!(
            problem_notice("the cache is unreadable", "run doctor"),
            "llmenv: the cache is unreadable. Fix: run doctor"
        );
    }
}
