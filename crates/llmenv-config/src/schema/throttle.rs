use serde::{Deserialize, Serialize};

fn default_throttle_cache_ttl() -> u64 {
    30
}
fn default_throttle_max_wait() -> u64 {
    300
}
fn default_throttle_soft_threshold() -> u64 {
    20
}

/// Usage throttling configuration. A tag-scoped entry that injects PreToolUse
/// and UserPromptSubmit hooks which poll the named backend and sleep a capped
/// adaptive delay to avoid hitting rate limits.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Throttle {
    /// Backend name that supplies usage data. Currently `"umans"` is the only
    /// supported value. An unknown backend is a no-op (with a stderr warning).
    pub backend: String,
    /// Tags that activate this throttle entry, intersected with active scope
    /// tags (same selection model as bundles, MCP servers, and memory).
    #[serde(default)]
    pub when: Vec<String>,
    /// How long (seconds) a fetched usage snapshot is cached on disk before
    /// the backend is polled again.
    #[serde(default = "default_throttle_cache_ttl")]
    pub cache_ttl: u64,
    /// Hard cap (seconds) on any single per-hook sleep.
    #[serde(default = "default_throttle_max_wait")]
    pub max_wait: u64,
    /// Remaining-request level at which adaptive delays begin.
    #[serde(default = "default_throttle_soft_threshold")]
    pub soft_threshold: u64,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn an_entry_with_only_a_backend_gets_the_default_limits() {
        let throttle: Throttle = serde_yaml::from_str("backend: umans\n").unwrap();
        assert_eq!(throttle.cache_ttl, 30);
        assert_eq!(throttle.max_wait, 300);
        assert_eq!(throttle.soft_threshold, 20);
        assert!(throttle.when.is_empty());
    }
}
