//! Timing instrumentation for hook operations (#3016).

use crate::hook_run::HookEvent;

/// Run the drift check with timing instrumentation.
///
/// This wraps `materialize::stale::report_if_stale` in a timing block and records the
/// result as a `check-stale` guard marker when tracing is on. Fail-soft: errors surface as an
/// agent notice, not a hook failure.
pub(crate) fn check_stale_with_timing(event: HookEvent, engine: &str) {
    if event != HookEvent::SessionStart || engine != "claude-code" {
        return;
    }
    let t_stale = std::time::Instant::now();
    let use_color = llmenv_util::should_use_color(None, false);
    if let Err(e) = crate::materialize::stale::report_if_stale(use_color) {
        crate::agent_notice::record(format!(
            "llmenv could not check whether your config drifted: {e:#}. \
             Fix: run `llmenv doctor` to see the cause."
        ));
    }
    crate::trace::emit_guard_trace("check-stale", t_stale.elapsed());
}
