//! Built-in MCP tool tiers shared by every adapter that renders tool permissions.
//!
//! A tier table names the tools of one MCP server by risk. Each adapter then turns the
//! table into its own permission syntax, so the default policy lives in one place.

use crate::config::McpPermissionAction;

/// Risk tier of one MCP tool (#946).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum McpTier {
    ReadOnly,
    Mutation,
    Destructive,
}

/// The action for one tier: the feature override when set, else the default.
///
/// Read-only and mutation tools default to `allow`; destructive tools default to `ask` (#946).
pub(super) fn tier_action(
    tier: McpTier,
    overrides: Option<&crate::config::McpPermissions>,
) -> McpPermissionAction {
    let configured = overrides.and_then(|o| match tier {
        McpTier::ReadOnly => o.read_only,
        McpTier::Mutation => o.mutation,
        McpTier::Destructive => o.destructive,
    });
    configured.unwrap_or(match tier {
        McpTier::ReadOnly | McpTier::Mutation => McpPermissionAction::Allow,
        McpTier::Destructive => McpPermissionAction::Ask,
    })
}

/// #694/#946: Built-in ICM MCP server tool tiers, rendered by
/// `apply_mcp_tier_permissions` into one coherent allow/ask/deny policy (no
/// wildcard/tier conflict — see #946). Default policy: read-only and
/// mutation tools → allow, destructive → ask; overridable per feature via
/// `features.memory[].mcp_permissions`.
pub(super) const ICM_READ_ONLY: &[&str] = &[
    "icm_wake_up",
    "icm_memory_recall",
    "icm_memory_related",
    "icm_memory_stats",
    "icm_memory_health",
    "icm_memory_list_topics",
    "icm_feedback_stats",
    "icm_feedback_search",
    "icm_transcript_search",
    "icm_transcript_stats",
    "icm_transcript_show",
    "icm_memoir_search",
    "icm_memoir_search_all",
    "icm_memoir_show",
    "icm_memoir_inspect",
    "icm_memoir_export",
    "icm_memoir_list",
];

pub(super) const ICM_MUTATION: &[&str] = &[
    "icm_memory_store",
    "icm_memory_update",
    "icm_memory_consolidate",
    "icm_memory_embed_all",
    "icm_memory_extract_patterns",
    "icm_learn",
    "icm_transcript_start_session",
    "icm_transcript_record",
    "icm_feedback_record",
    "icm_memoir_create",
    "icm_memoir_add_concept",
    "icm_memoir_refine",
    "icm_memoir_link",
];

pub(super) const ICM_DESTRUCTIVE: &[&str] = &["icm_memory_forget", "icm_memory_forget_topic"];

/// #946: Built-in context-mode MCP plugin tool tiers (without the common
/// `CONTEXT_MODE_MCP_PREFIX`), rendered the same way as the ICM tiers above.
pub(super) const CTX_READ_ONLY: &[&str] = &["ctx_search", "ctx_stats", "ctx_doctor", "ctx_insight"];

pub(super) const CTX_MUTATION: &[&str] = &[
    "ctx_index",
    "ctx_execute",
    "ctx_execute_file",
    "ctx_fetch_and_index",
    "ctx_batch_execute",
];

pub(super) const CTX_DESTRUCTIVE: &[&str] = &["ctx_purge", "ctx_upgrade"];

/// The context-mode tools that run code. They are also in `CTX_MUTATION`, which Claude Code
/// allows. opencode asks for them by default (see `opencode/tiers.rs`).
pub(super) const CTX_EXECUTE: &[&str] = &["ctx_execute", "ctx_execute_file", "ctx_batch_execute"];

/// #1323: Built-in codebase-memory-mcp server tool tiers, rendered the same
/// way as the ICM/context-mode tiers above. Source-verified against
/// `codebase-memory-mcp`'s own `TOOL_ANNOTATIONS` table (`src/mcp/mcp.c`),
/// which is a deliberately discriminating table, not a blanket MCP-spec
/// default: `index_repository` and `ingest_traces` are explicitly annotated
/// `destructive=false`, while `manage_adr` is explicitly annotated
/// `destructive=true, idempotent=false` — confirmed by its implementation,
/// an unversioned UPSERT (`cbm_store_adr_store`) that replaces the entire
/// ADR document with no history or backup, the one piece of human-authored
/// (not re-derivable-by-reindexing) state in the store. `manage_adr` is
/// therefore tiered `Destructive`, not `Mutation`, despite being a "write"
/// in the same broad sense as the other two.
///
/// Checked against cbm `v0.11.0`. `get_file_outline` is `ReadOnly` although
/// upstream annotates it destructive: its handler uses the query-only store
/// path and runs two `SELECT`s, and the annotation is a leftover of the
/// read-only split that was never revisited for this tool. `manage_adr`'s new
/// `set_sections` mode does not change its tier.
///
/// `delete_project` is also destructive (irreversibly removes a project's
/// index). The `ask` boundary alone doesn't cover everything destructive
/// here: `index_repository`'s `name` override can replace a *different*
/// project's index, and re-tiering `index_repository` to `ask` would defeat
/// the auto-index-on-`SessionStart` use case this tiering exists for. That
/// case is handled a layer down instead, by the `PreToolUse` deny in
/// [`crate::hook_run::cbm_index_guard`] (#1331).
pub(super) const CBM_READ_ONLY: &[&str] = &[
    "search_graph",
    "query_graph",
    "trace_path",
    "get_code_snippet",
    "get_file_outline",
    "get_graph_schema",
    "compare_graphs",
    "get_architecture",
    "search_code",
    "list_projects",
    "index_status",
    "check_index_coverage",
    "detect_changes",
];

pub(super) const CBM_MUTATION: &[&str] = &["index_repository", "ingest_traces"];

pub(super) const CBM_DESTRUCTIVE: &[&str] = &["delete_project", "manage_adr"];
