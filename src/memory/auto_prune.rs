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
        Err(e) => eprintln!("warning: auto_prune did not run: {e:#}"),
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
}
