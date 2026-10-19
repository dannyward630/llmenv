//! Tests of the entry that the guard reads (#2922).

use std::path::PathBuf;

use proptest::prelude::*;

use super::guard_decision_with;
use llmenv_config::CodebaseMemory;

fn tagged(when: &str, index_path: &str) -> CodebaseMemory {
    CodebaseMemory {
        when: vec![when.into()],
        index_path: Some(index_path.into()),
        ..Default::default()
    }
}

#[test]
fn the_guard_reads_only_the_entry_that_serves_the_session() {
    let confined = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let entries = [tagged("open", "/idx/open"), tagged("tight", "/idx/tight")];
    let root = confined.path().to_path_buf();
    let list = |cm: &CodebaseMemory| -> anyhow::Result<Vec<PathBuf>> {
        Ok(match cm.index_path.as_deref() {
            Some("/idx/tight") => vec![root.clone()],
            _ => vec![],
        })
    };
    let repo = elsewhere.path().display().to_string();
    let tags = |t: &str| std::collections::BTreeSet::from([t.to_string()]);
    assert_eq!(
        guard_decision_with(&repo, &entries, Some(&tags("open")), list),
        None,
        "the unconfined server of this session decides for itself"
    );
    assert!(guard_decision_with(&repo, &entries, Some(&tags("tight")), list).is_some());
    assert!(
        guard_decision_with(&repo, &entries, None, list).is_some(),
        "unknown scopes check every entry"
    );
    assert!(
        guard_decision_with(&repo, &entries, Some(&tags("none")), list).is_some(),
        "with no matching entry, as after a cd out of the project, every entry counts"
    );
    assert!(
        guard_decision_with(&repo, &[], None, list).is_some(),
        "without scopes an empty entry list proves nothing"
    );
    assert_eq!(
        guard_decision_with(&repo, &[], Some(&tags("open")), list),
        None
    );
}

proptest! {
    /// An entry whose tags are all inactive changes nothing while another entry is active.
    #[test]
    fn an_inactive_entry_does_not_change_the_decision(
        open_active in proptest::bool::ANY,
        extra_tag in "[c-f]",
    ) {
        let confined = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let root = confined.path().to_path_buf();
        let list = |cm: &CodebaseMemory| -> anyhow::Result<Vec<PathBuf>> {
            Ok(match cm.index_path.as_deref() {
                Some("/idx/tight") => vec![root.clone()],
                _ => vec![],
            })
        };
        let active = if open_active { "a" } else { "b" };
        let tags = std::collections::BTreeSet::from([active.to_string()]);
        let base = vec![tagged("a", "/idx/open"), tagged("b", "/idx/tight")];
        let mut more = base.clone();
        more.push(tagged(&extra_tag, "/idx/tight"));
        let repo = elsewhere.path().display().to_string();
        let before = guard_decision_with(&repo, &base, Some(&tags), list);
        let after = guard_decision_with(&repo, &more, Some(&tags), list);
        prop_assert_eq!(before.is_some(), after.is_some());
        prop_assert_eq!(before.is_none(), open_active);
    }
}
