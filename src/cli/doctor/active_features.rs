//! The `features` entries that apply in the active scopes, with the entries that firing bundles
//! declare. A doctor check that read every root entry reported tools and variables that no active
//! entry uses, and missed an entry that only a bundle declares (#2944).

use std::collections::BTreeMap;
use std::path::Path;

use crate::config::{CodebaseMemory, Config, HostEntry, Memory};
use crate::scope::ActiveScopes;

/// The active `memory` and `codebase_memory` entries, and the merged host table.
#[derive(Debug, Default)]
pub(super) struct ActiveFeatures {
    pub(super) memory: Vec<Memory>,
    host: BTreeMap<String, HostEntry>,
    pub(super) codebase_memory: Vec<CodebaseMemory>,
}

impl ActiveFeatures {
    /// The entries of the root config and the firing bundles, kept when their `when` tags are
    /// active. Endpoint resolution and the SessionStart index use the same rule.
    ///
    /// # Errors
    /// A firing bundle's `bundle.yaml` cannot be read.
    pub(super) fn resolve(
        config: &Config,
        config_dir: &Path,
        active: &ActiveScopes,
    ) -> anyhow::Result<Self> {
        let merged = crate::hook_run::merged_memory(config, config_dir, active)?;
        let codebase_memory =
            crate::hook_run::cbm_index_guard::codebase_memory_entries(config, config_dir, active)?;
        Ok(Self::active_only(
            merged.memory,
            merged.host,
            codebase_memory,
            active,
        ))
    }

    /// The root entries only, for a config whose bundles cannot be read.
    pub(super) fn root_only(config: &Config, active: &ActiveScopes) -> Self {
        let features = config.features.clone().unwrap_or_default();
        Self::active_only(
            features.memory,
            config.host.clone(),
            features.codebase_memory,
            active,
        )
    }

    fn active_only(
        memory: Vec<Memory>,
        host: BTreeMap<String, HostEntry>,
        codebase_memory: Vec<CodebaseMemory>,
        active: &ActiveScopes,
    ) -> Self {
        Self {
            memory: memory
                .into_iter()
                .filter(|m| crate::mcp::resolve::memory_is_tag_active(m, &active.tags))
                .collect(),
            host,
            codebase_memory: codebase_memory
                .into_iter()
                .filter(|cm| cm.when.iter().any(|t| active.tags.contains(t)))
                .collect(),
        }
    }

    /// Whether an active memory entry names a remote `server_host`: one that maps to a non-local
    /// address, or that the host table does not name (assumed remote).
    pub(super) fn has_remote_memory_host(&self) -> bool {
        self.memory.iter().any(|mem| {
            self.host
                .get(&mem.server_host)
                .is_none_or(|h| !super::is_local_addr(&h.addr))
        })
    }

    /// The `ANTHROPIC_MODEL` problem to show. Only an active, enabled memory entry on the
    /// `anthropic-api` backend reads the variable, so any other config gets no warning.
    pub(super) fn anthropic_model_warning(
        &self,
        model_var: Result<String, std::env::VarError>,
    ) -> Option<String> {
        let uses_api = self
            .memory
            .iter()
            .filter_map(|m| m.consolidation.as_ref())
            .any(|c| {
                c.enabled && matches!(c.backend, crate::config::ConsolidationBackend::AnthropicApi)
            });
        if !uses_api {
            return None;
        }
        crate::consolidation::model_problem(model_var).map(|problem| {
            format!(
                "{problem} This stops consolidation with the anthropic-api backend at every \
                 session end."
            )
        })
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests;
