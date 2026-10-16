//! Cut-item handling for the adaptive recall flows. icm cuts an item short, and llmenv
//! removes the cut line whole (#2756).

use super::*;

/// Drops every line that icm cut short. icm ends a cut item with the cut marker, and that item
/// lost its tail, so the line is removed whole rather than shown half-written (#2756). Returns
/// the kept text and how many lines were dropped.
pub(super) fn drop_cut_items(text: &str) -> (String, usize) {
    let (kept, cut): (Vec<&str>, Vec<&str>) = text
        .lines()
        .partition(|line| !line.trim_end().ends_with(CUT_MARKER));
    (kept.join("\n"), cut.len())
}

/// Bytes the wake-up pack takes from the output budget: the pack, or the project memories alone
/// when the wake-up call failed, plus the blank line that joins them to the scope set.
pub(super) fn pack_bytes(wake: &anyhow::Result<String>, own: &str) -> usize {
    wake.as_ref().map_or(own.len() + 2, |t| t.len() + 2)
}

/// The notice for wake-up items that icm cut short, or `None` when none were cut (#2756).
pub(super) fn cut_items_notice(count: usize) -> Option<String> {
    (count > 0).then(|| {
        format!(
            "llmenv: {count} memory item(s) were dropped because icm cut them short. Fix: store \
             shorter memories, or run `llmenv doctor` to check the memory server."
        )
    })
}

/// Tells the agent that icm cut some items short, so the drop is not silent (#2756).
pub(super) fn note_cut_items(count: usize) {
    if let Some(notice) = cut_items_notice(count) {
        agent_notice::record(notice);
    }
}

/// Joins the non-empty parts of the wake-up pack with a blank line.
pub(super) fn join_sections(first: &str, second: &str) -> String {
    [first, second]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
