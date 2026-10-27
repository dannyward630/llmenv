//! Per-phase timing for `llmenv export` (`llmenv-trace {"op":"export",...}`).
//!
//! These functions emit a JSON trace line with phase boundaries when
//! `LLMENV_TRACE_TIMING` is set. They are separated from `trace.rs` so that
//! `trace.rs` stays near its line cap.

use std::time::{Duration, Instant};

use crate::trace::emit_json_trace;

/// Emit a `llmenv-trace {"op":"export",...}` line with per-phase timing.
/// The fields are optional — a phase that did not run is left out.
pub(crate) fn emit_export_trace(
    t0: Instant,
    t_config: Instant,
    t_env: Option<Instant>,
    t_active: Option<Instant>,
    t_manifest: Option<Instant>,
    t_adapters: Option<Instant>,
    t_render: Option<Instant>,
) {
    let us = |d: Duration| u64::try_from(d.as_micros()).unwrap_or(u64::MAX);
    let mut fields = serde_json::Map::new();
    fields.insert(
        "config_load_us".to_string(),
        serde_json::json!(us(t_config.saturating_duration_since(t0))),
    );
    if let Some(t_env) = t_env {
        fields.insert(
            "detect_env_us".to_string(),
            serde_json::json!(us(t_env.saturating_duration_since(t_config))),
        );
    }
    if let Some(t_active) = t_active {
        fields.insert(
            "evaluate_active_us".to_string(),
            serde_json::json!(us(
                t_active.saturating_duration_since(t_env.unwrap_or(t_config))
            )),
        );
    }
    if let Some(t_manifest) = t_manifest {
        fields.insert(
            "build_manifest_us".to_string(),
            serde_json::json!(us(
                t_manifest.saturating_duration_since(t_active.unwrap_or(t_env.unwrap_or(t_config)))
            )),
        );
    }
    if let Some(t_adapters) = t_adapters {
        fields.insert(
            "materialize_adapters_us".to_string(),
            serde_json::json!(us(t_adapters.saturating_duration_since(
                t_manifest.unwrap_or(t_active.unwrap_or(t_env.unwrap_or(t_config)))
            ))),
        );
    }
    if let Some(t_render) = t_render {
        fields.insert(
            "render_vars_us".to_string(),
            serde_json::json!(us(t_render
                .saturating_duration_since(t_adapters.unwrap_or(
                    t_manifest.unwrap_or(t_active.unwrap_or(t_env.unwrap_or(t_config)))
                )))),
        );
    }
    emit_json_trace("export", serde_json::Value::Object(fields));
}
