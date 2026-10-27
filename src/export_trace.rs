//! Per-phase timing for `llmenv export` (`llmenv-trace {"op":"export",...}`).
//!
//! These functions emit a JSON trace line with phase boundaries when
//! `LLMENV_TRACE_TIMING` is set. They are separated from `trace.rs` so that
//! `trace.rs` stays near its line cap.

use std::time::{Duration, Instant};

use crate::trace::emit_json_trace;

/// Emit one `llmenv-trace {"op":<op>,...}` marker with per-phase timing.
///
/// `marks` lists each phase as `(field, end)` in run order. A phase measures from the latest
/// earlier end that ran, so a phase that did not run (`None`) is left out and does not break
/// the chain.
pub(crate) fn emit_phase_trace(op: &str, t0: Instant, marks: &[(&str, Option<Instant>)]) {
    emit_json_trace(op, serde_json::Value::Object(phase_fields(t0, marks)));
}

fn phase_fields(
    t0: Instant,
    marks: &[(&str, Option<Instant>)],
) -> serde_json::Map<String, serde_json::Value> {
    let us = |d: Duration| u64::try_from(d.as_micros()).unwrap_or(u64::MAX);
    let mut fields = serde_json::Map::new();
    let mut base = t0;
    for (name, end) in marks {
        if let Some(end) = end {
            fields.insert(
                (*name).to_string(),
                serde_json::json!(us(end.saturating_duration_since(base))),
            );
            base = *end;
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    // A skipped phase is left out, and the next phase measures from the last one that ran.
    #[test]
    fn a_skipped_phase_is_omitted_and_the_chain_continues() {
        let t0 = Instant::now();
        let t_a = t0 + Duration::from_micros(10);
        let t_c = t0 + Duration::from_micros(35);
        let fields = phase_fields(
            t0,
            &[("a_us", Some(t_a)), ("b_us", None), ("c_us", Some(t_c))],
        );
        assert_eq!(fields["a_us"], 10);
        assert_eq!(fields["c_us"], 25);
        assert!(!fields.contains_key("b_us"));
    }

    // The phases add up to the whole run when every phase ran.
    #[test]
    fn phases_sum_to_the_last_boundary() {
        let t0 = Instant::now();
        let marks = [
            ("a_us", Some(t0 + Duration::from_micros(7))),
            ("b_us", Some(t0 + Duration::from_micros(19))),
            ("c_us", Some(t0 + Duration::from_micros(40))),
        ];
        let sum: u64 = phase_fields(t0, &marks)
            .values()
            .filter_map(|v| v.as_u64())
            .sum();
        assert_eq!(sum, 40);
    }
}
