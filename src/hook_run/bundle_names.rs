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
    let disabled = crate::bundle_select::marker_disabled_bundle_names(active);
    let firing = crate::bundle_select::firing_bundles(bundles, active, None)
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

    // #1125: the recall-keyword/context-chunk bundle list is a second selection
    // that also ignored `disable_bundles`, so a bundle a project opts out of
    // still became an `llmenv-bundle:<name>` recall query and appeared in the
    // context chunk stored in the memory backend.
    #[test]
    fn recall_bundle_names_excludes_disabled_bundles() {
        let scope = |id: &str, enable: &[&str], disable: &[&str]| crate::scope::ActiveScope {
            id: id.to_string(),
            kind: "project",
            tags: vec![],
            project_root: None,
            enable_bundles: enable.iter().map(|s| (*s).to_string()).collect(),
            disable_bundles: disable.iter().map(|s| (*s).to_string()).collect(),
            name: None,
            description: None,
            unknown_fields: vec![],
        };

        let active = crate::scope::ActiveScopes {
            scopes: vec![
                scope("user", &["yaks", "rust-dev"], &[]),
                scope("project", &["web-dev"], &["yaks"]),
            ],
            ..Default::default()
        };

        assert_eq!(
            recall_bundle_names(&[], &active),
            vec!["rust-dev".to_string(), "web-dev".to_string()],
            "a disabled bundle must not reach recall queries or the stored chunk, \
                 even when another scope enables it"
        );
    }

    #[test]
    fn recall_bundle_names_dedupes_and_sorts() {
        let scope = |id: &str, enable: &[&str]| crate::scope::ActiveScope {
            id: id.to_string(),
            kind: "user",
            tags: vec![],
            project_root: None,
            enable_bundles: enable.iter().map(|s| (*s).to_string()).collect(),
            disable_bundles: vec![],
            name: None,
            description: None,
            unknown_fields: vec![],
        };

        let active = crate::scope::ActiveScopes {
            scopes: vec![scope("a", &["zed", "alpha"]), scope("b", &["alpha"])],
            ..Default::default()
        };

        assert_eq!(
            recall_bundle_names(&[], &active),
            vec!["alpha".to_string(), "zed".to_string()]
        );
    }

    #[test]
    fn a_top_level_bundle_that_fires_by_tag_is_named() {
        let bundles = vec![crate::config::Bundle {
            name: "web-dev".into(),
            when: vec!["web".into()],
        }];
        let active = crate::scope::ActiveScopes {
            scopes: vec![crate::scope::ActiveScope {
                id: "user".into(),
                kind: "user",
                tags: vec!["web".into()],
                project_root: None,
                enable_bundles: vec![],
                disable_bundles: vec![],
                name: None,
                description: None,
                unknown_fields: vec![],
            }],
            tags: std::iter::once("web".to_string()).collect(),
            ..Default::default()
        };
        assert_eq!(
            recall_bundle_names(&bundles, &active),
            vec!["web-dev".to_string()]
        );
    }
}
