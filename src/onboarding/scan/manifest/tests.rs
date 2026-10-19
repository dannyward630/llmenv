#![allow(clippy::unwrap_used)]

use std::os::fd::AsFd as _;

use proptest::prelude::*;

use super::{MAX_VALUE_CHARS, cut, describe};
use crate::paths::dirfd;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn cut_keeps_a_prefix_of_at_most_the_bound(value in "\\PC{0,400}") {
        let kept = cut(&value);
        prop_assert!(value.starts_with(&kept));
        prop_assert_eq!(kept.chars().count(), value.chars().count().min(MAX_VALUE_CHARS));
    }

    #[test]
    fn any_manifest_bytes_give_facts_or_a_note_and_never_panic(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
        name in prop::sample::select(vec!["Cargo.toml", "package.json", "pyproject.toml"]),
    ) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(name), &bytes).unwrap();
        let fd = dirfd::open_dir_nofollow(dir.path()).unwrap();

        let repo = describe(fd.as_fd(), dir.path());

        let parsed = match name {
            "Cargo.toml" => repo.cargo.is_some(),
            "package.json" => repo.package_json.is_some(),
            _ => repo.pyproject.is_some(),
        };
        let noted = repo.notes.iter().any(|n| n.contains(name));
        prop_assert!(parsed || noted, "{} gave no facts and no note: {:?}", name, repo.notes);
    }
}
