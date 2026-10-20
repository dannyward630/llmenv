//! The `auto_prune` pass that runs during materialize (#270, #2941, #2947).

/// Whether the active memory entry asks for `auto_prune`. `memory` is the merged list of the root
/// config and the firing bundles, the list that picks the memory server. Only one entry can be
/// active: with two, the server does not resolve and nothing is pruned. An inactive entry names a
/// backend that this scope does not use, so its flag must not prune the active one (#2941).
fn wants_auto_prune(
    memory: &[crate::config::Memory],
    active_tags: &std::collections::BTreeSet<String>,
) -> bool {
    single_active(memory, active_tags).is_some_and(|entry| entry.auto_prune)
}

/// The one active entry of `memory`, or `None` with zero or two or more: with two, the memory
/// server does not resolve, so no entry of them applies.
pub(crate) fn single_active<'a>(
    memory: &'a [crate::config::Memory],
    active_tags: &std::collections::BTreeSet<String>,
) -> Option<&'a crate::config::Memory> {
    let mut active = memory
        .iter()
        .filter(|m| crate::mcp::resolve::memory_is_tag_active(m, active_tags));
    match (active.next(), active.next()) {
        (Some(entry), None) => Some(entry),
        _ => None,
    }
}

/// The warning for an `auto_prune: true` that cannot run because more than one memory entry is
/// active, or `None`.
fn ambiguous_notice(
    memory: &[crate::config::Memory],
    active_tags: &std::collections::BTreeSet<String>,
) -> Option<String> {
    let active: Vec<_> = memory
        .iter()
        .filter(|m| crate::mcp::resolve::memory_is_tag_active(m, active_tags))
        .collect();
    (active.len() > 1 && active.iter().any(|m| m.auto_prune)).then(|| {
        format!(
            "warning: auto_prune did not run: {} memory entries are active, so no memory server \
             is picked. Fix: change the `when` tags so that one entry is active",
            active.len()
        )
    })
}

/// Run an automatic prune pass during materialize if the active memory entry has
/// `auto_prune: true`. `active` is the scope set of the render, so with `--scope` the check and
/// the prune use the same entry and the same server (#2947). Fail-soft: errors are printed as
/// warnings, never returned.
pub(crate) fn auto_prune_if_enabled(
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    active: &crate::scope::ActiveScopes,
) {
    let merged = match crate::memory::merged_memory(config, config_dir, active) {
        Ok(merged) => merged,
        Err(e) => {
            eprintln!(
                "warning: auto_prune did not run: the memory entries cannot be read: {e:#}. Fix \
                 the bundle that the error names, or set auto_prune to false"
            );
            return;
        }
    };
    if let Some(warning) = ambiguous_notice(&merged.memory, &active.tags) {
        eprintln!("{warning}");
        return;
    }
    if !wants_auto_prune(&merged.memory, &active.tags) {
        return;
    }

    tracing::info!("auto_prune: running memory prune pass during materialize");
    match super::prune::run_with(config, config_dir, active, &merged.memory, false) {
        Ok(result) => {
            tracing::info!(
                "auto_prune: forgot {} record(s) ({} total evaluated)",
                result.forgotten,
                result.total
            );
            if let Some(kept) = result.kept_unplaced() {
                eprintln!("warning: auto_prune kept {kept}");
            }
        }
        // `tracing` warnings are off at the default log level, and the user must learn that
        // the configured prune did not run.
        Err(e) => eprintln!(
            "warning: auto_prune did not run: {e:#}. Run `llmenv doctor` to check the memory \
             server, or set auto_prune to false"
        ),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn only_an_active_entry_can_turn_on_auto_prune() {
        let entry = |when: &str, auto_prune: bool| -> crate::config::Memory {
            serde_json::from_value(serde_json::json!({
                "when": [when], "server_host": "h", "port": 9092, "auto_prune": auto_prune
            }))
            .unwrap()
        };
        let memory = vec![entry("home", false), entry("work", true)];
        let tags = |t: &str| std::collections::BTreeSet::from([t.to_string()]);
        assert!(
            !wants_auto_prune(&memory, &tags("home")),
            "the inactive work entry decides nothing"
        );
        assert!(wants_auto_prune(&memory, &tags("work")));
        assert!(!wants_auto_prune(&memory, &tags("none")));
    }

    fn scopes(tag: &str) -> crate::scope::ActiveScopes {
        crate::scope::ActiveScopes {
            scopes: vec![crate::scope::ActiveScope {
                id: "host".to_string(),
                kind: "host",
                tags: vec![tag.to_string()],
                ..crate::scope::ActiveScope::default()
            }],
            tags: [tag.to_string()].into(),
            ..crate::scope::ActiveScopes::default()
        }
    }

    /// The check reads the same merged list that picks the server, so an entry that only a
    /// firing bundle declares turns the prune on, and a scope set narrowed by `--scope` decides
    /// with its own entry (#2947).
    #[test]
    fn a_bundle_entry_decides_for_the_scope_set_it_fires_in() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("bundles").join("mem");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("bundle.yaml"),
            "host:\n  b: { addr: 127.0.0.1 }\nfeatures:\n  memory:\n    - when: [work]\n      \
             server_host: b\n      port: 9092\n      auto_prune: true\n",
        )
        .unwrap();
        let config = crate::config::Config {
            bundle: vec![crate::config::Bundle {
                name: "mem".to_string(),
                when: vec!["work".to_string()],
            }],
            features: Some(crate::config::Features {
                memory: vec![memory(&[0], false)],
                ..Default::default()
            }),
            host: [(
                "h".to_string(),
                crate::config::HostEntry {
                    addr: "127.0.0.1".into(),
                },
            )]
            .into(),
            ..Default::default()
        };
        let decide = |tag: &str| {
            let active = scopes(tag);
            let merged = crate::memory::merged_memory(&config, root.path(), &active).unwrap();
            wants_auto_prune(&merged.memory, &active.tags)
        };
        assert!(decide("work"), "the bundle-only entry asks for the prune");
        assert!(
            !decide("a"),
            "the root entry without auto_prune is never pruned"
        );
    }

    const TAGS: [&str; 3] = ["a", "b", "c"];

    fn memory(when: &[usize], auto_prune: bool) -> crate::config::Memory {
        let when: Vec<&str> = when.iter().map(|&i| TAGS[i]).collect();
        serde_json::from_value(serde_json::json!({
            "when": when, "server_host": "h", "port": 9092, "auto_prune": auto_prune
        }))
        .unwrap()
    }

    proptest::proptest! {
        #[test]
        fn an_inactive_entry_never_changes_the_answer(
            entries in proptest::collection::vec(
                (proptest::collection::vec(0usize..3, 1..3), proptest::bool::ANY),
                0..5,
            ),
            active in proptest::collection::btree_set(0usize..3, 0..3),
            extra_prune in proptest::bool::ANY,
        ) {
            let active: std::collections::BTreeSet<String> =
                active.iter().map(|&i| TAGS[i].to_string()).collect();
            let live: Vec<bool> = entries
                .iter()
                .filter(|(when, _)| when.iter().any(|&i| active.contains(TAGS[i])))
                .map(|(_, prune)| *prune)
                .collect();
            let expected = live == [true];
            let mut memory: Vec<_> = entries.iter().map(|(w, p)| self::memory(w, *p)).collect();
            proptest::prop_assert_eq!(wants_auto_prune(&memory, &active), expected);
            let inactive: Vec<usize> = (0..3).filter(|&i| !active.contains(TAGS[i])).collect();
            if !inactive.is_empty() {
                memory.push(self::memory(&inactive, extra_prune));
                proptest::prop_assert_eq!(wants_auto_prune(&memory, &active), expected);
            }
        }
    }

    #[test]
    fn two_active_entries_name_the_auto_prune_that_did_not_run() {
        let entry = |auto_prune: bool| -> crate::config::Memory {
            serde_json::from_value(serde_json::json!({
                "when": ["t"], "server_host": "h", "port": 9092, "auto_prune": auto_prune
            }))
            .unwrap()
        };
        let tags = std::collections::BTreeSet::from(["t".to_string()]);
        let notice = ambiguous_notice(&[entry(false), entry(true)], &tags).unwrap();
        assert!(notice.contains("2 memory entries are active"), "{notice}");
        assert_eq!(ambiguous_notice(&[entry(false), entry(false)], &tags), None);
        assert_eq!(ambiguous_notice(&[entry(true)], &tags), None);
        assert!(single_active(&[entry(true), entry(true)], &tags).is_none());
    }
}
