//! Rank of each recall tag by how specific its scope is. Lower rank runs first.

use std::collections::BTreeMap;

/// The rank of a tag no scope contributed, such as the OS tag.
pub(in crate::hook_run) const UNSCOPED_RANK: u8 = 6;

/// The rank of every tag from a project scope or `$LLMENV_EXTRA_TAGS`, and of every bundle.
pub(in crate::hook_run) const MOST_SPECIFIC_RANK: u8 = 1;

/// Rank each active tag by how specific its source scope is. A lower rank is more specific
/// and is recalled first. A tag from several scopes takes the lowest rank among them.
pub(in crate::hook_run) fn tag_specificity(
    active: &crate::scope::ActiveScopes,
) -> BTreeMap<String, u8> {
    let mut ranks: BTreeMap<String, u8> = active
        .tags
        .iter()
        .map(|tag| (tag.clone(), UNSCOPED_RANK))
        .collect();
    let mut lower = |tag: &String, rank: u8| {
        ranks
            .entry(tag.clone())
            .and_modify(|current| *current = (*current).min(rank))
            .or_insert(rank);
    };
    for scope in &active.scopes {
        let rank = match scope.kind {
            "project" => 1,
            "content" => 2,
            "network" => 3,
            "user" => 4,
            "host" => 5,
            _ => UNSCOPED_RANK,
        };
        scope.tags.iter().for_each(|tag| lower(tag, rank));
    }
    // `extra_tags` stand in for a project's `.llmenv.yaml`, so they rank as project tags.
    active
        .extra_tags
        .iter()
        .for_each(|tag| lower(tag, MOST_SPECIFIC_RANK));
    ranks
}
