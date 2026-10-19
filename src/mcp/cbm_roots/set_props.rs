//! Property tests for the root set logic: containment, the missing and undeclared roots, and the
//! guard's outside check (#2924). Each property compares against a reference written here.

#![expect(clippy::expect_used, reason = "test scaffolding")]

use std::path::{Path, PathBuf};

use proptest::prelude::*;

use super::{is_within_any, missing_roots, path_outside_roots, undeclared_roots};

/// Folders of the test tree, relative to its root. Some nest, so containment has both answers.
const FOLDERS: [&str; 6] = ["a", "a/x", "a/x/deep", "b", "c", "c/y"];
/// Names that do not exist, so the "does not resolve" paths are covered too.
const ABSENT: [&str; 2] = ["gone", "a/gone"];

/// Build the tree and return its canonical root.
fn tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    for folder in FOLDERS {
        std::fs::create_dir_all(dir.path().join(folder)).expect("mkdir");
    }
    let root = std::fs::canonicalize(dir.path()).expect("canonical");
    (dir, root)
}

fn pick() -> impl Strategy<Value = Vec<&'static str>> {
    let names: Vec<&str> = FOLDERS.iter().chain(ABSENT.iter()).copied().collect();
    proptest::sample::subsequence(names.clone(), 0..=names.len())
}

fn exists(root: &Path, rel: &str) -> bool {
    FOLDERS.contains(&rel) && root.join(rel).is_dir()
}

/// The reference containment: both sides exist, and the root is a component prefix.
fn within(root: &Path, path: &str, roots: &[&str]) -> bool {
    exists(root, path)
        && roots
            .iter()
            .any(|r| exists(root, r) && Path::new(path).starts_with(r))
}

fn paths(root: &Path, rels: &[&str]) -> Vec<PathBuf> {
    rels.iter().map(|r| root.join(r)).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn containment_matches_the_component_prefix_rule(path in pick(), roots in pick()) {
        let (_dir, root) = tree();
        let listed = paths(&root, &roots);
        for rel in &path {
            prop_assert_eq!(
                is_within_any(&root.join(rel), &listed),
                within(&root, rel, &roots),
                "{} in {:?}", rel, roots
            );
        }
        prop_assert!(path.iter().all(|rel| !is_within_any(&root.join(rel), &[])));
    }

    #[test]
    fn missing_roots_are_the_declared_folders_that_no_record_covers(
        wanted in pick(),
        listed in pick(),
    ) {
        let (_dir, root) = tree();
        let got = missing_roots(&paths(&root, &wanted), &paths(&root, &listed));
        let expected: Vec<PathBuf> = if listed.is_empty() {
            Vec::new()
        } else {
            wanted
                .iter()
                .filter(|w| exists(&root, w) && !within(&root, w, &listed))
                .map(|w| root.join(w))
                .collect()
        };
        prop_assert_eq!(got, expected);
    }

    #[test]
    fn undeclared_roots_are_the_records_that_no_declared_root_covers(
        listed in pick(),
        wanted in pick(),
    ) {
        let (_dir, root) = tree();
        let got = undeclared_roots(&paths(&root, &listed), &paths(&root, &wanted));
        let expected: Vec<PathBuf> = listed
            .iter()
            .filter(|l| exists(&root, l) && !within(&root, l, &wanted))
            .map(|l| root.join(l))
            .collect();
        prop_assert_eq!(got, expected);
    }

    /// The guard check is pure: a path is inside when a root is a component prefix of it.
    #[test]
    fn the_guard_denies_exactly_the_paths_outside_every_root(
        path in proptest::collection::vec("[ab]{1,2}", 1..4),
        roots in proptest::collection::vec(proptest::collection::vec("[ab]{1,2}", 0..3), 0..4),
    ) {
        let path = PathBuf::from("/").join(path.join("/"));
        let roots: Vec<PathBuf> = roots.iter().map(|r| PathBuf::from("/").join(r.join("/"))).collect();
        let inside = roots.iter().any(|r| path.starts_with(r));
        let reason = path_outside_roots(&path, &roots);
        prop_assert_eq!(reason.is_none(), inside);
        if let Some(reason) = reason {
            prop_assert!(reason.contains(&path.display().to_string()), "{}", reason);
        }
    }
}
