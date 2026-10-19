//! The records of an ICM recall text, and the records that a recall across all projects
//! leaves out.

/// The topic of the scope-context record that SessionEnd stores. The topic names no project, so
/// only a recall across all projects returns it.
pub(super) const SCOPE_CONTEXT_TOPIC: &str = "llmenv-scope-context";

/// `text` without the records that belong to one project but carry no project in their topic:
/// fetched pages (#2911), and scope-context records, which describe the tags and bundles of the
/// project that stored them (#2938).
pub(super) fn without_unscoped_records(text: &str) -> String {
    let pages_dropped = crate::hook_run::web_fetch::without_web_pages(text);
    split_recall_records(&pages_dropped)
        .into_iter()
        .filter(|record| {
            crate::hook_run::relevance::record_topic(record) != Some(SCOPE_CONTEXT_TOPIC)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Longest topic, in characters, that a record's first line may carry.
pub(super) const MAX_TOPIC_CHARS: usize = 200;

/// Whether `line` opens an ICM recall record: `[<topic>] <text>`.
///
/// Phases: the line starts with `[`; the first `]` ends the topic; the topic has 1 to
/// [`MAX_TOPIC_CHARS`] characters; a space follows the `]`.
pub(super) fn is_record_start(line: &str) -> bool {
    let Some(after_open) = line.strip_prefix('[') else {
        return false;
    };
    let Some((topic, after_topic)) = after_open.split_once(']') else {
        return false;
    };
    (1..=MAX_TOPIC_CHARS).contains(&topic.chars().count()) && after_topic.starts_with(' ')
}

/// Split one recall action's text into ICM records, so a byte budget can keep
/// whole records and drop the rest (#2159).
///
/// A record starts at a `[<topic>] ` line and runs to the next such line. Lines
/// before the first record form a record of their own. Text with no record
/// line is one record, so a change to ICM's format degrades to per-action
/// behavior and never loses text. Each record has trailing whitespace removed,
/// and empty records are dropped.
pub(crate) fn split_recall_records(text: &str) -> Vec<String> {
    let mut records: Vec<Vec<&str>> = Vec::new();
    for line in text.lines() {
        match records.last_mut() {
            Some(current) if !is_record_start(line) => current.push(line),
            _ => records.push(vec![line]),
        }
    }
    records
        .into_iter()
        .map(|lines| lines.join("\n").trim_end().to_string())
        .filter(|record| !record.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn arb_records() -> impl Strategy<Value = Vec<(String, String)>> {
        proptest::collection::vec(
            (
                prop_oneof![
                    Just(SCOPE_CONTEXT_TOPIC.to_string()),
                    "[a-z]{1,8}",
                    "[a-zé日]{1,4}",
                ],
                "[a-z][a-z ]{0,12}[a-z]",
            ),
            0..6,
        )
    }

    proptest! {
        /// The filter drops the scope-context records and keeps every other record, in order.
        #[test]
        fn only_the_scope_context_records_are_dropped(records in arb_records()) {
            let text: Vec<String> = records.iter().map(|(t, b)| format!("[{t}] {b}")).collect();
            let expected: Vec<&String> = records
                .iter()
                .zip(&text)
                .filter(|((t, _), _)| t != SCOPE_CONTEXT_TOPIC)
                .map(|(_, line)| line)
                .collect();
            let kept = without_unscoped_records(&text.join("\n"));
            let kept: Vec<&str> = kept.lines().collect();
            prop_assert_eq!(kept, expected.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        }

        /// A record line carries a topic of 1 to 200 characters, counted as characters, not
        /// bytes, so a topic in a script with wide characters is not cut short.
        #[test]
        fn the_topic_bound_counts_characters(n in 0usize..=203, wide in any::<bool>()) {
            let c = if wide { "日" } else { "a" };
            let line = format!("[{}] body", c.repeat(n));
            prop_assert_eq!(is_record_start(&line), (1..=MAX_TOPIC_CHARS).contains(&n));
        }
    }
}
