//! A trace line must match its entry in `website/docs/trace.md` (#3018).
//!
//! The check compares the shape of a line: its tag, the `key=` names in order, and the position
//! of each duration. A rename, a new field, a removed field, or a reorder fails a test until the
//! docs page shows the same shape.

use std::time::Duration;

use crate::trace::SpawnExit;
use crate::trace::lines::{self, DetachedStats, LedgerFields};

const DOCS: &str = include_str!("../../website/docs/trace.md");

/// The tag and each `key=` name in `text`, with a duration shown as `#us` or `#ms` and any other
/// value shown as `_`. Square brackets that mark an optional field are dropped.
pub(crate) fn shape(text: &str) -> Vec<String> {
    let mut parts = text.split_whitespace();
    let tag = parts.next().unwrap_or_default().to_string();
    let rest: String = parts.collect::<Vec<_>>().join(" ").replace(['[', ']'], "");
    let mut shape = vec![tag];
    for token in rest.split_whitespace() {
        shape.push(shape_of_token(token));
    }
    shape
}

fn shape_of_token(token: &str) -> String {
    if let Some((key, _)) = token.split_once('=') {
        return format!("{key}=");
    }
    for unit in ["us", "ms"] {
        let Some(value) = token.strip_suffix(unit) else {
            continue;
        };
        let is_number = !value.is_empty() && value.chars().all(|c| c.is_ascii_digit() || c == '.');
        if is_number || (value.starts_with('<') && value.ends_with('>')) {
            return format!("#{unit}");
        }
    }
    "_".to_string()
}

/// Every `[LLMENV_*]` template that the docs page shows in a fenced `text` block.
fn doc_templates() -> Vec<String> {
    let mut templates = Vec::new();
    let mut in_text_block = false;
    for line in DOCS.lines() {
        if line.starts_with("```") {
            in_text_block = line == "```text";
            continue;
        }
        if in_text_block && line.starts_with("[LLMENV_") {
            templates.push(line.to_string());
        }
    }
    templates
}

/// The shapes a template allows: one with its optional `[...]` group, and one without it.
fn template_shapes(template: &str) -> Vec<Vec<String>> {
    let (tag, rest) = template.split_once(' ').unwrap_or((template, ""));
    let Some((before, after)) = rest.split_once('[') else {
        return vec![shape(template)];
    };
    let after = after.split_once(']').map_or("", |(_, tail)| tail);
    vec![shape(template), shape(&format!("{tag} {before}{after}"))]
}

/// Whether the docs page shows a template with the same shape as `line`.
pub(crate) fn docs_show(line: &str) -> bool {
    let wanted = shape(line);
    doc_templates()
        .iter()
        .any(|t| template_shapes(t).contains(&wanted))
}

/// One sample of every line the binary emits, except `[LLMENV_CONTEXT]`. The recall tests check
/// that line, because its builder is private to `hook_run`.
fn samples() -> Vec<(&'static str, String)> {
    let took = Duration::from_micros(1234);
    let wrote = llmenv_paths::WriteStats {
        files: 3,
        bytes: 99,
        skipped: 4,
    };
    let detached = DetachedStats {
        wall_us: 5,
        cpu_us: 6,
        mcp_calls: 7,
        exit: 0,
    };
    let ledger = LedgerFields {
        lock_attempts: Some(1),
        lock_wait_us: Some(2),
        read_us: Some(3),
        write_us: Some(4),
        bytes: Some(5),
        state_unsaved: Some(false),
    };
    vec![
        (
            "cache hit",
            lines::cache_line("merge_sig", true, took, None),
        ),
        (
            "cache miss",
            lines::cache_line("merge_sig", false, took, Some("abc")),
        ),
        (
            "spawn",
            lines::spawn_line("probe:ssid", took, SpawnExit::Code(0)),
        ),
        ("ledger update", lines::ledger_update_line("update", took)),
        (
            "ledger flow",
            lines::ledger_flow_line("session_start", ledger),
        ),
        (
            "mcp tool",
            lines::mcp_call_line("icm_memory_recall", took, 10, 20, None),
        ),
        (
            "mcp handshake",
            lines::mcp_call_line("initialize", took, 10, 20, Some(500)),
        ),
        (
            "materialize",
            lines::materialize_line("claude-code", wrote, took),
        ),
        (
            "detached",
            lines::detached_line("icm-store", detached, Some("abc123")),
        ),
    ]
}

#[test]
fn every_emitted_line_has_a_matching_docs_entry() {
    for (name, line) in samples() {
        assert!(
            docs_show(&line),
            "website/docs/trace.md shows no template for the `{name}` line `{line}`.\n\
             Update the page so a template has the same tag, keys, and order."
        );
    }
}

#[test]
fn every_docs_template_matches_an_emitted_line() {
    let emitted: Vec<Vec<String>> = samples().iter().map(|(_, line)| shape(line)).collect();
    for template in doc_templates() {
        let context = template.starts_with("[LLMENV_CONTEXT]");
        // The full form of a template, with every optional field, must be emitted.
        let full = shape(&template);
        assert!(
            context || emitted.contains(&full),
            "website/docs/trace.md shows a template that no code emits: `{template}`"
        );
    }
}

#[test]
fn the_docs_show_every_json_marker_op() {
    for op in ["hook_run", "export", "regenerate", "statusline", "guard"] {
        assert!(
            DOCS.contains(&format!("\"op\": \"{op}\"")),
            "website/docs/trace.md has no example for the `{op}` marker"
        );
    }
}

#[test]
fn a_changed_line_shape_is_not_shown_by_the_docs() {
    let line = lines::spawn_line("probe:ssid", Duration::from_micros(1), SpawnExit::Code(0));
    assert!(docs_show(&line));
    assert!(
        !docs_show(&line.replace("exit=", "status=")),
        "a renamed key must not match"
    );
    assert!(
        !docs_show(&format!("{line} extra=1")),
        "an added key must not match"
    );
}
