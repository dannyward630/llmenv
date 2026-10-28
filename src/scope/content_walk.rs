//! Content-scope matching: one walk of the working directory for every content scope (#703).

use crate::config::ContentScope;

/// Log a walk error. An error at the root means no entry can be read, so it is an error. An error
/// below the root skips one entry, so it is a warning (#3070).
fn report_walk_error(err: &walkdir::Error, cwd: &std::path::Path) {
    if err.depth() == 0 {
        tracing::error!(
            cwd = %cwd.display(),
            error = %err,
            "content scopes cannot read the working directory, so none of them match. Fix: check \
             the permissions of the working directory."
        );
    } else {
        tracing::warn!(error = %err, "walkdir entry error; skipping");
    }
}

/// Each content scope previously triggered its own `walkdir` traversal
/// (#703) — N active content scopes meant N full tree walks on every hook
/// fire and export. Here all globs are compiled up front and evaluated
/// per entry against a single walk; a scope drops out of the pending set as
/// soon as it matches, and the walk ends early once none remain pending.
///
/// Returns the `id`s of scopes whose glob matched.
///
/// A working directory that cannot be read gives no matches. That case is logged at error, so
/// the empty result has a visible cause (#3070).
#[must_use]
pub(crate) fn matches_content_all<'a>(
    scopes: &'a [ContentScope],
    cwd: &std::path::Path,
) -> std::collections::BTreeSet<&'a str> {
    let mut pending: Vec<(&str, globset::GlobMatcher, Option<usize>)> = scopes
        .iter()
        .filter_map(|s| match globset::Glob::new(&s.r#match.glob) {
            Ok(glob) => Some((s.id.as_str(), glob.compile_matcher(), s.r#match.depth)),
            Err(_) => {
                tracing::debug!("content scope {}: invalid glob pattern", s.id);
                None
            }
        })
        .collect();

    let mut matched = std::collections::BTreeSet::new();
    if pending.is_empty() {
        return matched;
    }

    // Cap the walk at the loosest per-scope depth limit; any scope with no
    // limit forces an unbounded walk (short-circuits to `None` below).
    let max_depth = pending
        .iter()
        .map(|(_, _, d)| *d)
        .try_fold(0usize, |acc, d| d.map(|d| acc.max(d)));

    let mut walker = walkdir::WalkDir::new(cwd).follow_links(false);
    if let Some(depth) = max_depth {
        walker = walker.max_depth(depth);
    }

    for entry in walker {
        if pending.is_empty() {
            break;
        }
        let Ok(entry) = entry.inspect_err(|e| report_walk_error(e, cwd)) else {
            continue;
        };
        if entry.file_type().is_dir() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(cwd) else {
            // walkdir only yields paths under root, so this is a walkdir bug
            debug_assert!(
                false,
                "walkdir path {:?} not under root {:?}",
                entry.path(),
                cwd,
            );
            tracing::warn!(
                path = ?entry.path(),
                cwd = ?cwd,
                "walkdir yielded path outside root; skipping",
            );
            continue;
        };
        let entry_depth = entry.depth();
        pending.retain(|(id, matcher, depth)| {
            if depth.is_some_and(|d| entry_depth > d) {
                return true;
            }
            if matcher.is_match(relative) {
                matched.insert(*id);
                false
            } else {
                true
            }
        });
    }
    matched
}
