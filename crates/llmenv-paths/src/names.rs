//! Checks on path names and path strings, done on the text alone, with no filesystem call.

use std::path::{Path, PathBuf};

/// True if `path` contains any parent (`..`) component, parsed
/// component-wise rather than by substring. Catches traversal that string
/// matching misses: `foo/..`, mixed separators on the host OS, and a bare
/// `..` with no trailing slash. A leading `/` (root) is fine; only `..`
/// components are rejected.
///
/// Note: this does NOT check whether `path` is absolute. `Path::join` with
/// an absolute argument returns the argument unchanged, escaping the base
/// directory. When validating relative paths supplied by user-controlled
/// data, use [`is_unsafe_join_target`] instead.
#[must_use]
pub fn has_parent_component(path: &str) -> bool {
    use std::path::Component;
    Path::new(path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

/// True if joining `path` onto a base directory would escape it. Returns
/// true when `path` contains `..` components OR is absolute (since
/// `Path::join` with an absolute argument discards the base). Use this at
/// every site that does `base.join(user_controlled_rel)`.
#[must_use]
pub fn is_unsafe_join_target(path: &str) -> bool {
    let p = Path::new(path);
    p.is_absolute() || has_parent_component(path)
}

/// True if `name` is safe to use as a single path component (a marketplace,
/// skill, or plugin-collection name) and as a JSON key — ASCII
/// alphanumeric plus `.`/`_`/`-`, not empty, not `.`/`..`, not leading with
/// `-` (git/CLI arg-parsing hazard). Rejects everything a component-based
/// blocklist could miss (control characters, Unicode formatting characters
/// like zero-width space or RTL override, path separators) by construction,
/// rather than by enumerating what to reject (#534).
#[must_use]
pub fn is_valid_short_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.starts_with('-') {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Return true if `cwd` is at or below `prefix`, treating both as filesystem
/// paths (component-wise) rather than raw strings. This avoids the
/// `/home/alice/git/xyz` matches prefix `/home/alice/git/x` bug.
#[must_use]
pub fn cwd_under_prefix(cwd: &str, prefix: &str) -> bool {
    let cwd_p = Path::new(cwd);
    let pre_p = PathBuf::from(prefix);
    cwd_p.starts_with(&pre_p)
}
