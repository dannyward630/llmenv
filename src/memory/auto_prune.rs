//! The `auto_prune` pass that runs during materialize (#270, #2941).

/// Whether an active memory entry asks for `auto_prune`. An inactive entry names a backend that
/// this scope does not use, so its flag must not prune the active one (#2941).
fn wants_auto_prune(
    config: &crate::config::Config,
    active_tags: &std::collections::BTreeSet<String>,
) -> bool {
    config.features.as_ref().is_some_and(|f| {
        f.memory
            .iter()
            .any(|m| m.auto_prune && crate::mcp::resolve::memory_is_tag_active(m, active_tags))
    })
}

/// Run an automatic prune pass during materialize if an active memory entry has
/// `auto_prune: true`. Fail-soft: errors are logged as warnings, never
/// returned.
pub(crate) fn auto_prune_if_enabled(
    config: &crate::config::Config,
    active_tags: &std::collections::BTreeSet<String>,
) {
    if !wants_auto_prune(config, active_tags) {
        return;
    }

    tracing::info!("auto_prune: running memory prune pass during materialize");
    match super::prune::run(false) {
        Ok(result) => {
            tracing::info!(
                "auto_prune: forgot {} record(s) ({} total evaluated)",
                result.forgotten,
                result.total
            );
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
        let config = crate::config::Config {
            features: Some(crate::config::Features {
                memory: vec![entry("home", false), entry("work", true)],
                ..Default::default()
            }),
            ..Default::default()
        };
        let tags = |t: &str| std::collections::BTreeSet::from([t.to_string()]);
        assert!(
            !wants_auto_prune(&config, &tags("home")),
            "the inactive work entry decides nothing"
        );
        assert!(wants_auto_prune(&config, &tags("work")));
        assert!(!wants_auto_prune(&config, &tags("none")));
    }

    const TAGS: [&str; 3] = ["a", "b", "c"];

    fn memory(when: &[usize], auto_prune: bool) -> crate::config::Memory {
        let when: Vec<&str> = when.iter().map(|&i| TAGS[i]).collect();
        serde_json::from_value(serde_json::json!({
            "when": when, "server_host": "h", "port": 9092, "auto_prune": auto_prune
        }))
        .unwrap()
    }

    fn config_of(memory: Vec<crate::config::Memory>) -> crate::config::Config {
        crate::config::Config {
            features: Some(crate::config::Features {
                memory,
                ..Default::default()
            }),
            ..Default::default()
        }
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
            let expected = entries.iter().any(|(when, prune)| {
                *prune && when.iter().any(|&i| active.contains(TAGS[i]))
            });
            let mut memory: Vec<_> = entries.iter().map(|(w, p)| self::memory(w, *p)).collect();
            proptest::prop_assert_eq!(wants_auto_prune(&config_of(memory.clone()), &active), expected);
            let inactive: Vec<usize> = (0..3).filter(|&i| !active.contains(TAGS[i])).collect();
            if !inactive.is_empty() {
                memory.push(self::memory(&inactive, extra_prune));
                proptest::prop_assert_eq!(wants_auto_prune(&config_of(memory), &active), expected);
            }
        }
    }
}
