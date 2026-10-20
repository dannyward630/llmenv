//! The failure recall: which failures skip it (#2910), and which memories it keeps (#2921).
//!
//! A typo (exit 127) or a parse error in the agent's own script matches memories from every
//! project. A recall for such a failure injects unrelated notes, so the recall is skipped.

use std::collections::BTreeSet;

use anyhow::Context;
use serde::Deserialize;

use crate::hook_run::action::{Action, RecallQuery};
use crate::hook_run::mcp_client::McpHttpClient;

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
