//! The failure recall: which failures skip it (#2910), and which memories it keeps (#2921).
//!
//! A typo (exit 127) or a parse error in the agent's own script matches memories from every
//! project. A recall for such a failure injects unrelated notes, so the recall is skipped.

use std::collections::BTreeSet;

use anyhow::Context;
use serde::Deserialize;

use super::{
    AdaptiveCtx, FAILURE_BUDGET_BYTES, FAILURE_LIMIT, FANOUT_LIMIT, Outcome, RECALL_STATE_UNSAVED,
    RESOLVED_TOPIC, agent_key, note_skipped, note_unless_saved, query, report,
};
use crate::hook_run::action::{Action, RecallQuery};
use crate::hook_run::mcp_client::McpHttpClient;
use crate::hook_run::recall::RecallBudget;
use crate::hook_run::relevance;
use crate::hook_run::session_state::unix_now;

/// Words that appear in most failures and in most memories, so a shared one shows no relation.
const STOP_WORDS: [&str; 40] = [
    "after", "before", "cannot", "code", "command", "could", "error", "errors", "exit", "expected",
    "fail", "failed", "failure", "false", "file", "files", "found", "from", "have", "help", "into",
    "line", "none", "note", "null", "path", "result", "returned", "running", "status", "test",
    "tests", "that", "then", "there", "this", "true", "unable", "warning", "with",
];

/// One memory of a JSON recall reply.
#[derive(Debug, Deserialize)]
struct Memory {
    topic: String,
    summary: String,
    #[serde(default)]
    keywords: Vec<String>,
}

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
    // Only the first line names the failure. A long log of a real failure can quote a marker
    // further down, and that failure is not generic.
    let first = error
        .lines()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    GENERIC_MARKERS.iter().any(|marker| first.contains(marker))
}

/// Recalls `query` for a failure, and keeps only the memories that share a distinctive token
/// with `error`. `None` means no recall ran.
///
/// ICM normalizes its score per query, so the best of an unrelated set also scores near 1.0
/// and cannot gate the injection. A raw score is requested upstream (rtk-ai/icm#521).
pub(super) async fn recall(
    client: &McpHttpClient,
    query: Option<RecallQuery>,
    error: &str,
) -> Option<anyhow::Result<String>> {
    let action = Action::RecallQuery(query?);
    let mut args = action.arguments("", "");
    args["format"] = serde_json::json!("json");
    let reply = client.call_tool(action.tool_name(), args).await;
    // The capped query text without the tool name: "bash" would tie every shell note to the
    // failure, and a long log would tie almost anything.
    let error = crate::hook_run::relevance::error_query("", error);
    Some(reply.and_then(|body| related_records(&body, &error)))
}

/// The memories of a JSON recall reply that share a distinctive token with `error`, as
/// `[topic] summary` records.
///
/// # Errors
/// Fails when the reply does not start with a JSON list of memories.
fn related_records(body: &str, error: &str) -> anyhow::Result<String> {
    // Read the first JSON value only, so a note that icm appends after the list is ignored.
    let memories: Vec<Memory> = serde_json::Deserializer::from_str(body.trim())
        .into_iter()
        .next()
        .context("the reply is empty")?
        .with_context(|| {
            let start: String = body.chars().take(80).collect();
            format!("the failure recall reply is not a JSON list of memories: {start:?}")
        })?;
    let wanted = distinctive_tokens(error);
    let records: Vec<String> = memories
        .iter()
        .filter(|m| {
            let text = format!("{} {}", m.summary, m.keywords.join(" "));
            !distinctive_tokens(&text).is_disjoint(&wanted)
        })
        .map(|m| format!("[{}] {}", m.topic, m.summary))
        .collect();
    Ok(records.join("\n"))
}

/// The lowercase tokens of `text` that can tie a memory to a failure: a word of four or more
/// characters that is not a stop word, or an error code such as `E0063`.
pub(super) fn distinctive_tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map(str::to_lowercase)
        .filter(|t| is_distinctive(t))
        .collect()
}

fn is_distinctive(token: &str) -> bool {
    // A number alone is a line, a count, or a year, so it names no failure.
    if !token.chars().any(char::is_alphabetic) {
        return false;
    }
    // An error code such as E0063 or ENOENT2 is ASCII, so `len` counts its characters.
    let is_code = token.is_ascii() && token.len() >= 3 && token.bytes().any(|b| b.is_ascii_digit());
    is_code || (token.chars().count() >= 4 && !STOP_WORDS.contains(&token))
}

/// `PostToolUseFailure`: record the error, then inject related memories once.
pub(in crate::hook_run) async fn tool_failure(
    ctx: &AdaptiveCtx<'_>,
) -> anyhow::Result<crate::hook_run::HookText> {
    let tool = ctx.payload["tool_name"].as_str().unwrap_or_default();
    let error = ctx.payload["error"].as_str().unwrap_or_default();
    let now = unix_now();
    // One lock covers the error record and the read. Without the read, a busy or
    // unreadable ledger would drop the record with no log (#2723).
    let Some(ledger) = ctx.store.update(ctx.session_id, |l| {
        l.push_error(tool, error, now);
        l.clone()
    }) else {
        tracing::error!(
            "tool_failure: session {} tool {tool}: error not recorded and recall skipped, \
             the recall ledger cannot be updated (unsafe session id, busy lock, or unreadable file)",
            ctx.session_id
        );
        note_skipped();
        return Ok(crate::hook_run::HookText::default());
    };
    // A generic failure matches memories from every project, so no recall runs (#2910).
    if is_generic(error) {
        return Ok(crate::hook_run::HookText::default());
    }
    let (key, record) = agent_key(ctx.payload);
    let sent = ledger.sent_for(&key);
    let text = relevance::error_query(tool, error);
    // The session project comes first, as in `waves`. The shared `errors-resolved` topic is
    // the only cross-project recall here, and only while `global_topics` lists it (#2950). No
    // all-projects recall runs on a failure (#2910).
    let resolved = ctx
        .global_topics
        .iter()
        .any(|t| t.trim() == RESOLVED_TOPIC)
        .then(|| RecallQuery {
            topic: Some(RESOLVED_TOPIC.to_string()),
            ..query(&text, FANOUT_LIMIT)
        });
    let scoped = ledger.project.as_deref().map(|p| RecallQuery {
        project: Some(p.to_string()),
        ..query(&text, FAILURE_LIMIT)
    });
    // The all-projects recall is not run here: it injected other projects' notes (#2910).
    let (own, fixes) = tokio::join!(
        recall(ctx.client, scoped, error),
        recall(ctx.client, resolved, error),
    );
    let mut outcome = Outcome::default();
    outcome.push(own);
    outcome.push(fixes);
    let mut budget = RecallBudget::new(FAILURE_BUDGET_BYTES, sent);
    outcome.texts.iter().for_each(|t| budget.add_text(t));
    report(ctx, "post_tool_use_failure", &outcome, &budget);
    if record {
        let kept = budget.kept_hashes();
        note_unless_saved(
            ctx.store
                .update(ctx.session_id, |l| l.mark_sent(&key, kept)),
            RECALL_STATE_UNSAVED,
        );
    }
    Ok(budget.hook_text(Vec::new()))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use proptest::prelude::*;

    use super::{distinctive_tokens, is_generic, related_records};

    fn reply(items: &[(&str, &str, &[&str])]) -> String {
        let list: Vec<serde_json::Value> = items
            .iter()
            .map(|(t, s, k)| serde_json::json!({"topic": t, "summary": s, "keywords": k}))
            .collect();
        serde_json::Value::Array(list).to_string()
    }

    #[test]
    fn tokens_keep_words_and_codes_and_drop_noise() {
        let tokens =
            distinctive_tokens("Exit code 101\nerror[E0063]: missing field `request_ctx` in 2026");
        let got: Vec<&str> = tokens.iter().map(String::as_str).collect();
        assert_eq!(got, ["e0063", "field", "missing", "request_ctx"]);
    }

    #[test]
    fn a_memory_needs_a_shared_token_in_its_summary_or_keywords() {
        let body = reply(&[
            ("a", "Semgrep rules need a pinned ruleset", &["semgrep"]),
            ("b", "pin mcp below 2", &["request_ctx"]),
            ("c", "the E0063 fix", &[]),
        ]);
        let out = related_records(&body, "error[E0063]: missing request_ctx").unwrap();
        assert_eq!(out, "[b] pin mcp below 2\n[c] the E0063 fix");
    }

    #[test]
    fn only_stop_words_in_common_is_no_relation() {
        let body = reply(&[("a", "the command failed with an error", &["error"])]);
        assert_eq!(related_records(&body, "Error: command failed").unwrap(), "");
    }

    #[test]
    fn an_empty_list_and_a_trailing_note_are_read() {
        assert_eq!(related_records("[]", "boom").unwrap(), "");
        let body = format!(
            "{}\n\n[ICM: 9 tool calls since last store.]",
            reply(&[("a", "boom fix", &[])])
        );
        assert_eq!(related_records(&body, "boom").unwrap(), "[a] boom fix");
    }

    #[test]
    fn a_text_reply_is_an_error_that_names_the_reply() {
        let err = related_records("[errors-resolved] a note", "boom").unwrap_err();
        assert!(format!("{err:#}").contains("not a JSON list"), "{err:#}");
        assert!(related_records("", "boom").is_err());
    }

    proptest! {
        /// Every token is a lowercase word of the input, has a letter, and has three or more
        /// characters.
        #[test]
        fn every_token_is_a_long_lowercase_word(text in "\\PC{0,120}") {
            let lower = text.to_lowercase();
            for t in distinctive_tokens(&text) {
                prop_assert_eq!(t.to_lowercase(), t.clone());
                prop_assert!(lower.contains(t.as_str()));
                prop_assert!(t.chars().any(char::is_alphabetic));
                prop_assert!(t.chars().all(|c| c.is_alphanumeric() || c == '_'));
                prop_assert!(t.chars().count() >= 3);
            }
        }

        /// A memory that repeats the error always shares a token with it, when the error has one.
        #[test]
        fn a_memory_that_quotes_the_error_is_kept(error in "[a-zA-Z0-9_ :]{0,60}") {
            let body = reply(&[("t", &error, &[])]);
            let out = related_records(&body, &error).unwrap();
            prop_assert_eq!(out.is_empty(), distinctive_tokens(&error).is_empty());
        }
    }

    #[test]
    fn a_typo_and_a_script_parse_error_are_generic() {
        assert!(is_generic("Exit code 127\nzsh: command not found: gti"));
        assert!(is_generic(
            "SyntaxError: Unexpected token } in JSON at position 4"
        ));
    }

    #[test]
    fn a_marker_deep_in_a_long_log_is_not_generic() {
        let log = "Exit code 1\nrunning 40 tests\nthe parser test printed:\nSyntaxError: bad input";
        assert!(!is_generic(log));
    }

    #[test]
    fn a_failing_test_is_not_generic() {
        assert!(!is_generic(
            "Exit code 1\nthread 'parse' panicked at src/lib.rs:4"
        ));
        assert!(!is_generic(""));
    }
}
