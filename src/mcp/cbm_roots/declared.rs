//! The roots that the config declares, expanded, and the problems in them (#2406, #2916).
//!
//! A declared root is an entry of `allowed_roots`. It is the only source of a root that llmenv
//! expects the server to hold. A session folder or a llmenv directory is not one.

use std::path::{Path, PathBuf};

use crate::config::CodebaseMemory;
use crate::mcp::cbm_roots::RootBases;

/// Replace `~` and `$VAR` or `${VAR}` in `entry`. `None` when a variable is unset or empty, or
/// when `~` needs a home directory that is not known.
fn expand_root(
    entry: &str,
    home: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<PathBuf> {
    let entry = entry.trim();
    let mut out = String::new();
    let rest = if let Some(after) = entry.strip_prefix('~') {
        if !(after.is_empty() || after.starts_with('/')) {
            // `~user` is not supported.
            return None;
        }
        // A non-UTF-8 home has no text form. `to_string_lossy` would write U+FFFD into the
        // path, so the entry is unresolvable instead (#2600).
        out.push_str(home?.to_str()?);
        after
    } else {
        entry
    };
    out.push_str(&crate::util::expand_env_refs(rest, env)?);
    Some(PathBuf::from(out))
}

/// Whether `path` is too broad to allow as the project folder: the filesystem root, the home
/// folder, or a parent of it. The server keeps an allowed root for good, so a session started in
/// such a folder would leave every project below it open to `index_repository`.
fn is_broad_root(path: &Path, home: Option<&Path>) -> bool {
    path.parent().is_none() || home.is_some_and(|h| h.starts_with(path))
}

/// The roots that `cm` declares, expanded, without duplicates (#2406). An entry whose variable is
/// unset is dropped. Nothing else is a root: not the session folder, not a llmenv directory
/// (#2916).
fn configured_roots_in(
    cm: &CodebaseMemory,
    bases: &RootBases,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in cm
        .allowed_roots
        .iter()
        .filter_map(|entry| expand_root(entry, bases.home.as_deref(), env))
    {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

/// [`configured_roots_in`] with the environment of this process.
pub(crate) fn configured_roots(cm: &CodebaseMemory, bases: &RootBases) -> Vec<PathBuf> {
    configured_roots_in(cm, bases, &crate::env_var::utf8_var_or_log)
}

/// The problems in the roots that the user wrote or started a session in, as lines to show:
/// an entry that cannot expand (an unset variable or `~user`), an entry that is not a folder, and
/// a project folder that is too broad to allow. A default root may be absent, but a root the
/// user wrote is a typo or a missing mount.
pub(crate) fn root_problems(cm: &CodebaseMemory, bases: &RootBases) -> Vec<String> {
    root_problems_in(cm, bases, &crate::env_var::utf8_var_or_log)
}

fn root_problems_in(
    cm: &CodebaseMemory,
    bases: &RootBases,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let mut unexpandable = Vec::new();
    let mut absent = Vec::new();
    for entry in &cm.allowed_roots {
        match expand_root(entry, bases.home.as_deref(), env) {
            None => unexpandable.push(entry.as_str()),
            Some(root) if root.is_absolute() && !root.is_dir() => {
                absent.push(root.display().to_string());
            }
            Some(_) => {}
        }
    }
    if !unexpandable.is_empty() {
        problems.push(format!(
            "codebase-memory: allowed_roots entries that cannot expand (an unset or empty \
             variable, or ~user): {}. Fix the entry in the llmenv config.",
            unexpandable.join(", ")
        ));
    }
    if !absent.is_empty() {
        problems.push(format!(
            "codebase-memory: allowed_roots entries that are not folders: {}. Fix the entry in \
             the llmenv config.",
            absent.join(", ")
        ));
    }
    if is_broad_root(&bases.project_root, bases.home.as_deref()) {
        problems.push(format!(
            "codebase-memory: the session folder {} is too broad to allow, so it is not an \
             allowed root. Start the session in a project folder, or add the folders to index \
             to allowed_roots.",
            bases.project_root.display()
        ));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn bases() -> RootBases {
        RootBases {
            project_root: "/work/proj".into(),
            home: Some("/home/u".into()),
        }
    }

    fn cm(roots: &[&str]) -> CodebaseMemory {
        CodebaseMemory {
            when: vec!["p".into()],
            allowed_roots: roots.iter().map(|r| (*r).to_string()).collect(),
            ..Default::default()
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn strings(roots: Vec<PathBuf>) -> Vec<String> {
        roots.into_iter().map(|r| r.display().to_string()).collect()
    }

    #[test]
    fn no_default_root_is_declared() {
        assert!(configured_roots_in(&cm(&[]), &bases(), &no_env).is_empty());
    }

    #[test]
    fn variables_expand_and_an_unset_one_drops_the_entry() {
        let env = |name: &str| (name == "WORK").then(|| "/mnt/work".to_string());
        let roots = strings(configured_roots_in(
            &cm(&[
                "$WORK/a",
                "${WORK}/b",
                "$MISSING/c",
                "${MISSING}",
                "~other/x",
            ]),
            &bases(),
            &env,
        ));
        assert_eq!(roots, ["/mnt/work/a", "/mnt/work/b"]);
    }

    #[test]
    fn a_tilde_entry_expands_under_home_and_drops_without_home() {
        let roots = strings(configured_roots_in(&cm(&["~/notes"]), &bases(), &no_env));
        assert_eq!(roots, ["/home/u/notes"]);
        let mut b = bases();
        b.home = None;
        assert!(configured_roots_in(&cm(&["~/x"]), &b, &no_env).is_empty());
    }

    #[test]
    fn duplicates_are_removed_and_an_empty_variable_counts_as_unset() {
        let env = |name: &str| (name == "EMPTY").then(String::new);
        let roots = strings(configured_roots_in(
            &cm(&["/srv/x", "/srv/x", "$EMPTY/y"]),
            &bases(),
            &env,
        ));
        assert_eq!(roots, ["/srv/x"]);
    }

    #[cfg(unix)]
    #[test]
    fn expand_root_with_a_non_utf8_home_is_unresolvable_not_lossy() {
        use std::os::unix::ffi::OsStrExt as _;
        let home = std::ffi::OsStr::from_bytes(b"/home/\x80u");
        // A U+FFFD path would point at a directory that does not exist (#2600).
        assert_eq!(expand_root("~/code", Some(Path::new(home)), &no_env), None);
    }

    #[test]
    fn a_broad_project_folder_is_a_problem_and_a_project_folder_is_not() {
        for project in ["/", "/home", "/home/u"] {
            let mut b = bases();
            b.project_root = project.into();
            let problems = root_problems_in(&cm(&[]), &b, &no_env);
            assert!(
                problems.iter().any(|p| p.contains("too broad")),
                "{project}: {problems:?}"
            );
        }
        assert!(root_problems_in(&cm(&[]), &bases(), &no_env).is_empty());
    }

    #[test]
    fn an_entry_that_cannot_expand_is_reported_not_dropped_silently() {
        let problems =
            root_problems_in(&cm(&["$MISSING/c", "~other/x", "/srv"]), &bases(), &no_env);
        let text = problems.join("\n");
        assert!(
            text.contains("$MISSING/c") && text.contains("~other/x"),
            "{text}"
        );
        assert!(text.contains("cannot expand"), "{text}");
    }

    #[test]
    fn an_entry_that_is_not_a_folder_is_reported() {
        let problems = root_problems_in(&cm(&["/no/such/folder/x"]), &bases(), &no_env);
        assert!(
            problems.iter().any(|p| p.contains("/no/such/folder/x")),
            "{problems:?}"
        );
    }

    proptest! {
        #[test]
        fn expanding_never_panics_and_leaves_no_dollar(
            entry in "\\PC{0,40}",
        ) {
            let env = |name: &str| Some(format!("/v/{name}"));
            if let Some(path) = expand_root(&entry, Some(Path::new("/home/u")), &env) {
                let text = path.to_string_lossy().into_owned();
                prop_assert!(!text.contains('$'), "{text}");
            }
        }

        /// `~` and `~/rest` expand against the given home, `~name` is refused, and an entry
        /// with no `~` and no `$` is the trimmed entry itself (#2924).
        #[test]
        fn expanding_follows_the_tilde_rules(
            rest in "(/[a-z]{1,5}){0,3}",
            name in "[a-z]{1,5}",
            plain in "/[a-z]{1,5}(/[a-z]{1,5}){0,2}",
        ) {
            let home = Path::new("/home/u");
            prop_assert_eq!(
                expand_root(&format!("~{rest}"), Some(home), &no_env),
                Some(PathBuf::from(format!("/home/u{rest}")))
            );
            prop_assert_eq!(expand_root(&format!("~{name}{rest}"), Some(home), &no_env), None);
            prop_assert_eq!(expand_root(&format!("~{rest}"), None, &no_env), None);
            prop_assert_eq!(
                expand_root(&format!("  {plain} "), Some(home), &no_env),
                Some(PathBuf::from(&plain))
            );
        }

        #[test]
        fn declared_roots_never_repeat(
            extra in prop::collection::vec("/[a-z]{1,5}(/[a-z]{1,5}){0,2}", 0..5),
        ) {
            let c = cm(&extra.iter().map(String::as_str).collect::<Vec<_>>());
            let roots = configured_roots_in(&c, &bases(), &no_env);
            let unique: std::collections::BTreeSet<_> = roots.iter().collect();
            prop_assert_eq!(unique.len(), roots.len());
        }
    }
}
