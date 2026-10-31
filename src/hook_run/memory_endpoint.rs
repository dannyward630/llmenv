//! Memory-backend endpoint resolution for hook events: which backend the active scope names,
//! and the cached client that talks to it.

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::{Mutex, OnceLock};

use super::mcp_client::McpHttpClient;
use super::{HOOK_TIMEOUT, memory_url};
use crate::mcp::resolve::MemoryHookSettings;

/// A resolved memory-backend client plus the active `features.memory`
/// entry's hook settings (#1216, #2249) — the two travel together since both
/// come from the same resolved endpoint.
pub(super) struct ResolvedMemoryClient {
    pub(super) client: McpHttpClient,
    pub(super) settings: MemoryHookSettings,
}

/// Resolve (or reuse from `cache`) the MCP client for the active memory
/// backend, for lifecycle-hook events. Returns `Option`, never `Result`: no
/// cause of an unresolved backend — including a bundle-merge failure
/// (#1132) — is fatal to the hook event. Memory actions are simply skipped;
/// session logging (independent of the memory backend) still proceeds. A
/// caller that instead wrote `memory_url(...)?.into_url()` would propagate
/// `memory_url`'s own `Err` via that leading `?` before `.into_url()` ever
/// ran, silently reintroducing the abort this function exists to prevent
/// (#1139).
pub(super) fn resolve_memory_client(
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    active: &crate::scope::ActiveScopes,
    event: impl std::fmt::Display,
    cache: &'static OnceLock<Mutex<HashMap<String, McpHttpClient>>>,
) -> Option<ResolvedMemoryClient> {
    let endpoint = match memory_url(config, config_dir, active) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            // A failed bundle merge means a backend may be configured but unreadable. Hook stderr
            // never reaches the agent, so the reason is also a notice (#3202).
            crate::agent_notice::record_logged(format!(
                "llmenv: memory {event} skipped: the memory backend failed to load: {e}. Fix: \
                 correct the bundle or config that declares features.memory."
            ));
            return None;
        }
    };
    let (url, settings) = match endpoint.into_url_and_settings() {
        Ok(pair) => pair,
        Err(e) => {
            // Silent for the agent on purpose. The scope has no backend, a bundle turns the
            // backend off, or a fired bundle has no content, so its declaration cannot be read.
            // A notice here would repeat on every event for such a config. A content-less bundle
            // that does declare memory is therefore not reported to the agent.
            let _ = writeln!(std::io::stderr(), "llmenv: memory {event} skipped: {e}");
            return None;
        }
    };
    let clients = cache.get_or_init(|| Mutex::new(HashMap::new()));
    let mut clients = clients.lock().unwrap_or_else(|e| e.into_inner());
    let client = match clients.entry(url) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.get().clone(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            match McpHttpClient::new(entry.key().clone(), HOOK_TIMEOUT) {
                Ok(client) => entry.insert(client).clone(),
                Err(e) => {
                    // Hook stderr never reaches the agent, so the reason is also a notice (#3129).
                    crate::agent_notice::record_logged(format!(
                        "llmenv: memory {event} skipped: invalid memory backend URL: {e}. Fix: \
                         correct the memory URL in the config."
                    ));
                    return None;
                }
            }
        }
    };
    Some(ResolvedMemoryClient { client, settings })
}
