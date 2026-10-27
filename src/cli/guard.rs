//! Guard command dispatch: config-guard, config-context, check-stale, throttle.
//!
//! These commands run on every tool call, outside `hook-run`. This module
//! handles their dispatch from the CLI match arm.

/// Run a guard command with trace timing. Wraps the call in Instant
/// tracking and emits the guard trace marker.
fn run_with_guard_timing<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let t0 = std::time::Instant::now();
    let result = f();
    let t_config = std::time::Instant::now();
    crate::trace::emit_guard_trace_timing(t0, t_config, None);
    result
}

/// Run a guard command with trace timing, tracking scope separately.
fn run_with_guard_timing_scope<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let t0 = std::time::Instant::now();
    let result = f();
    let t_config = std::time::Instant::now();
    let t_scope = std::time::Instant::now();
    crate::trace::emit_guard_trace_timing(t0, t_config, Some(t_scope));
    result
}

/// Dispatch `check-stale`. Called from the CLI `run` match arm.
pub(super) fn dispatch_check_stale(
    use_color: bool,
    auto_fix: bool,
    engine: &str,
) -> anyhow::Result<()> {
    dispatch_guard_command(engine, "check-stale");
    run_with_guard_timing_scope(|| crate::cli::run_check_stale(use_color, auto_fix))
}

/// Dispatch `config-context`. Called from the CLI `run` match arm.
pub(super) fn dispatch_config_context(engine: &str) {
    dispatch_guard_command(engine, "config-context");
    run_with_guard_timing(crate::cli::run_config_context);
}

/// Dispatch `config-guard`. Called from the CLI `run` match arm.
pub(super) fn dispatch_config_guard(engine: &str) {
    dispatch_guard_command(engine, "config-guard");
    run_with_guard_timing(|| {
        crate::cli::config_guard::run(engine);
    });
}

/// Dispatch `throttle`. Called from the CLI `run` match arm.
pub(super) fn dispatch_throttle(event: &str) {
    dispatch_guard_command(event, "throttle");
    run_with_guard_timing(|| {
        crate::throttle::run_throttle_hook(event);
    });
}

/// Log a guard command invocation and warn about unknown engines.
fn dispatch_guard_command(engine: &str, name: &str) {
    tracing::debug!(engine, "{name} invoked by engine");
    crate::cli::warn_if_unknown_engine(engine);
}
