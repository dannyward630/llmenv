//! The `PreToolUse` guard of `index_repository`: the call must stay inside the roots that the
//! server of this session recorded (#2406, #2916, #2922).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{canonical, list_roots};
use llmenv_config::CodebaseMemory;

/// The `repo_path` of an `index_repository` call that is outside the allowed roots, as the
/// reason to deny it. `None` when it is inside one. Both sides must be canonical, so a link into
/// a root passes and a `..` escape does not (#2406).
pub(super) fn path_outside_roots(repo_path: &Path, roots: &[PathBuf]) -> Option<String> {
    if roots.iter().any(|root| repo_path.starts_with(root)) {
        return None;
    }
    let list = roots
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "{} is outside the codebase-memory allowed roots ({list}). Ask the user to record it: \
         `codebase-memory-mcp allow-root <path>`, with CBM_CACHE_DIR set to the entry's \
         index_path when one is set. Do not run allow-root yourself.",
        repo_path.display()
    ))
}

/// The reason to deny an `index_repository` call, or `None` to let it pass (#2406). The allowed
/// set is what the server records, so a root that the user recorded counts. The entry whose
/// `when` tags are active serves the session, so only its roots count (#2922). The hook reads the
/// tags from its own folder, which a `cd` can move away from the session's project, so when no
/// entry matches, or the scopes cannot be read (`active_tags` is `None`), every entry counts, as
/// before #2922. The server's own root check stays the backstop.
///
/// `entries` must hold the entries of the root config and of the firing bundles, as export merges
/// them. With no entry, codebase memory is off and the guard does not apply.
pub fn guard_decision(
    repo_path: &str,
    entries: &[CodebaseMemory],
    active_tags: Option<&BTreeSet<String>>,
) -> Option<String> {
    guard_decision_with(repo_path, entries, active_tags, list_roots)
}

/// [`guard_decision`] with the root listing given, so a test needs no server.
pub(super) fn guard_decision_with(
    repo_path: &str,
    entries: &[CodebaseMemory],
    active_tags: Option<&BTreeSet<String>>,
    list: impl Fn(&CodebaseMemory) -> anyhow::Result<Vec<PathBuf>>,
) -> Option<String> {
    // Without scopes the config may also lack the entries that bundles add, so an empty list
    // proves nothing.
    if active_tags.is_none() && entries.is_empty() {
        return Some(format!(
            "{repo_path} cannot be checked: the scopes of this session could not be read, so its \
             codebase-memory entry is unknown. Ask the user to run `llmenv doctor`."
        ));
    }
    // Only one entry may be active (`resolve_codebase_memory_entries`). With two, both count.
    let mut serving: Vec<&CodebaseMemory> = entries
        .iter()
        .filter(|cm| active_tags.is_some_and(|tags| cm.when.iter().any(|t| tags.contains(t))))
        .collect();
    if serving.is_empty() {
        serving = entries.iter().collect();
    }
    let mut listed = Vec::new();
    let mut unreadable = Vec::new();
    for cm in serving {
        match list(cm) {
            Ok(roots) => listed.extend(roots),
            Err(e) => unreadable.push(format!("{} ({e:#})", entry_name(cm))),
        }
    }
    decide_with_reads(repo_path, listed, &unreadable)
}

/// How a message names an entry: its `index_path`, or the default cache.
fn entry_name(cm: &CodebaseMemory) -> String {
    cm.index_path.as_deref().map_or_else(
        || "the entry with the default index path".to_string(),
        |path| format!("the entry with index_path {path}"),
    )
}

/// [`guard_decision`] given the listed roots and the failed reads. An empty record with a failed
/// read is not unconfined: the read may have failed, so the guard denies (#2916).
pub(super) fn decide_with_reads(
    repo_path: &str,
    listed: Vec<PathBuf>,
    unreadable: &[String],
) -> Option<String> {
    if listed.is_empty() && !unreadable.is_empty() {
        return Some(format!(
            "{repo_path} cannot be checked against the codebase-memory allowed roots, because \
             they could not be read: {}. Ask the user to run `llmenv doctor`.",
            unreadable.join("; ")
        ));
    }
    decide(repo_path, listed).map(|reason| with_unreadable_note(reason, unreadable))
}

/// `reason` with the failures to read the server's recorded roots, so a deny that came from a
/// failed read does not look like a root the user forgot to add.
fn with_unreadable_note(reason: String, unreadable: &[String]) -> String {
    if unreadable.is_empty() {
        return reason;
    }
    format!(
        "{reason} The roots that the server recorded could not be read: {}.",
        unreadable.join("; ")
    )
}

/// [`guard_decision`] with the listed roots given. An empty record means the server is
/// unconfined, so the server decides and the guard passes (#2916). A `repo_path` that does not
/// exist is denied: the server cannot index it, and no canonical path exists to compare.
pub(super) fn decide(repo_path: &str, listed: Vec<PathBuf>) -> Option<String> {
    if listed.is_empty() {
        return None;
    }
    // A relative path resolves against the hook's folder, but the server resolves it against its
    // own, so the check would judge a different folder than the one indexed (#2916).
    if !Path::new(repo_path).is_absolute() {
        return Some(format!(
            "{repo_path} is not an absolute path, so it cannot be checked against the \
             codebase-memory allowed roots. Pass the absolute path of the repository."
        ));
    }
    let roots: Vec<PathBuf> = listed
        .iter()
        .filter_map(|root| {
            let resolved = canonical(root);
            if resolved.is_none() {
                tracing::error!(
                    "recorded root {} does not resolve, so it is skipped",
                    root.display()
                );
            }
            resolved
        })
        .collect();
    if roots.is_empty() {
        return Some(format!(
            "{repo_path} cannot be checked: none of the {} recorded roots resolves to a folder. \
             Ask the user to run `llmenv doctor`.",
            listed.len()
        ));
    }
    match std::fs::canonicalize(repo_path) {
        Ok(canonical) => path_outside_roots(&canonical, &roots),
        Err(e) => Some(format!(
            "{repo_path} cannot be indexed: it does not resolve to a folder ({e}). Pass the \
             path of an existing repository."
        )),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests;
