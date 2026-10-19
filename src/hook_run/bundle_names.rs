//! The bundle names that recall and the stored context chunk use (#1125, #2898).

/// Bundle names for recall keywords and the stored context chunk: the bundles
/// that fire for `active` (top-level `bundle:` entries, selected by their tags
/// or by an enable marker) plus every active scope's `enable_bundles`,
/// deduplicated and sorted, minus anything any scope disables. `disable_bundles`
/// wins here too (#1125): a project that opts out of a bundle must not have it
/// named in queries sent to the memory backend or in the context chunk stored
/// there. The firing set is the one `icm.json` shows (#2898). An `enable_bundles`
/// name that no bundle entry declares is kept too, as it was before, so a name
/// that cannot fire still appears in these lists.
pub(super) fn recall_bundle_names(
    bundles: &[crate::config::Bundle],
    active: &crate::scope::ActiveScopes,
) -> Vec<String> {
    let disabled = crate::cli::marker_disabled_bundle_names(active);
    let firing = crate::cli::firing_bundles(bundles, active, None)
        .into_iter()
        .map(|b| b.name.clone());
    active
        .scopes
        .iter()
        .flat_map(|s| s.enable_bundles.iter().cloned())
        .chain(firing)
        .filter(|b| !disabled.contains(b))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{ActiveScope, ActiveScopes};
    use proptest::prelude::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    /// One active scope of `kind` with the given tags and enable and disable lists.
    fn scope(kind: &'static str, tags: &[&str], enable: &[&str], disable: &[&str]) -> ActiveScope {
        ActiveScope {
            id: kind.to_string(),
            kind,
            tags: names(tags),
            enable_bundles: names(enable),
            disable_bundles: names(disable),
            ..ActiveScope::default()
        }
    }

    // #1125: the recall-keyword/context-chunk bundle list is a second selection
    // that also ignored `disable_bundles`, so a bundle a project opts out of
    // still became an `llmenv-bundle:<name>` recall query and appeared in the
    // context chunk stored in the memory backend.
    #[test]
    fn recall_bundle_names_excludes_disabled_bundles() {
        let active = ActiveScopes {
            scopes: vec![
                scope("user", &[], &["yaks", "rust-dev"], &[]),
                scope("project", &[], &["web-dev"], &["yaks"]),
            ],
            ..Default::default()
        };

        assert_eq!(
            recall_bundle_names(&[], &active),
            names(&["rust-dev", "web-dev"]),
            "a disabled bundle must not reach recall queries or the stored chunk, \
                 even when another scope enables it"
        );
    }

    #[test]
    fn recall_bundle_names_dedupes_and_sorts() {
        let active = ActiveScopes {
            scopes: vec![
                scope("user", &[], &["zed", "alpha"], &[]),
                scope("user", &[], &["alpha"], &[]),
            ],
            ..Default::default()
        };

        assert_eq!(recall_bundle_names(&[], &active), names(&["alpha", "zed"]));
    }

    #[test]
    fn a_top_level_bundle_that_fires_by_tag_is_named() {
        let bundles = vec![crate::config::Bundle {
            name: "web-dev".into(),
            when: vec!["web".into()],
        }];
        let active = ActiveScopes {
            scopes: vec![scope("user", &["web"], &[], &[])],
            tags: std::iter::once("web".to_string()).collect(),
            ..Default::default()
        };
        assert_eq!(recall_bundle_names(&bundles, &active), names(&["web-dev"]));
    }

    const BUNDLES: [&str; 4] = ["a", "b", "c", "d"];

    fn subset() -> impl Strategy<Value = Vec<&'static str>> {
        proptest::sample::subsequence(BUNDLES.to_vec(), 0..=BUNDLES.len())
    }

    #[test]
    fn a_bundle_that_fires_by_tag_but_a_scope_disables_is_not_named() {
        let bundles = vec![crate::config::Bundle {
            name: "web-dev".into(),
            when: vec!["web".into()],
        }];
        let active = ActiveScopes {
            scopes: vec![scope("project", &["web"], &[], &["web-dev"])],
            tags: std::iter::once("web".to_string()).collect(),
            ..Default::default()
        };
        assert!(recall_bundle_names(&bundles, &active).is_empty());
    }

    const TAGS: [&str; 3] = ["t1", "t2", "t3"];

    proptest! {
        /// The names are the enabled names and the bundles whose `when` meets an active tag,
        /// minus every name a scope disables: sorted and unique (#1125, #2924).
        #[test]
        fn names_are_the_fired_and_enabled_bundles_minus_the_disabled(
            scopes in proptest::collection::vec((subset(), subset()), 0..4),
            when in proptest::collection::vec(
                proptest::option::of(proptest::sample::select(TAGS.to_vec())),
                BUNDLES.len(),
            ),
            tags in proptest::sample::subsequence(TAGS.to_vec(), 0..=TAGS.len()),
        ) {
            let bundles: Vec<crate::config::Bundle> = BUNDLES
                .iter()
                .zip(&when)
                .map(|(name, tag)| crate::config::Bundle {
                    name: (*name).to_string(),
                    when: tag.iter().map(|t| (*t).to_string()).collect(),
                })
                .collect();
            let active = ActiveScopes {
                scopes: scopes
                    .iter()
                    .map(|(enable, disable)| scope("project", &[], enable, disable))
                    .collect(),
                tags: tags.iter().map(|t| (*t).to_string()).collect(),
                ..Default::default()
            };
            let disabled: std::collections::BTreeSet<&str> =
                scopes.iter().flat_map(|(_, d)| d.iter().copied()).collect();
            let fired = BUNDLES
                .iter()
                .zip(&when)
                .filter(|(_, tag)| tag.is_some_and(|t| tags.contains(&t)))
                .map(|(name, _)| *name);
            let expected: std::collections::BTreeSet<&str> = scopes
                .iter()
                .flat_map(|(e, _)| e.iter().copied())
                .chain(fired)
                .filter(|name| !disabled.contains(name))
                .collect();

            let got = recall_bundle_names(&bundles, &active);

            prop_assert_eq!(got, expected.into_iter().map(String::from).collect::<Vec<_>>());
        }
    }
}
