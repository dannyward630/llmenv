//! MCP tool tiers as opencode permission rules (#2847).
//!
//! opencode names an MCP tool `<server>_<tool>`. Each entry of a tier table becomes one
//! rule on that name. The tier rules come before the user's rules, so a user rule still
//! wins under opencode's last-match order.

use super::permissions::Rule;
use crate::adapter::mcp_tiers::{
    CBM_DESTRUCTIVE, CBM_MUTATION, CBM_READ_ONLY, CTX_DESTRUCTIVE, CTX_EXECUTE, CTX_MUTATION,
    CTX_READ_ONLY, ICM_DESTRUCTIVE, ICM_MUTATION, ICM_READ_ONLY, McpTier, tier_action,
};
use crate::adapter::tools::opencode_hooks::mcp_namespace;
use crate::config::{McpPermissionAction, McpPermissions};
use crate::mcp::resolve::{CODEBASE_MEMORY_MCP_NAME, MEMORY_MCP_NAME, ResolvedMcp};
use crate::merge::MergedManifest;

/// The context-mode server key in opencode. Its Claude tool prefix,
/// `mcp__plugin_context-mode_context-mode__`, names the same server.
const CONTEXT_MODE_SERVER: &str = "context-mode";

/// The opencode permission rules for the tool tiers of every feature-enabled MCP server.
pub(super) fn tier_rules(manifest: &MergedManifest) -> Vec<Rule> {
    let mut rules = Vec::new();
    if let Some(icm) = find_mcp(manifest, MEMORY_MCP_NAME) {
        push_server_rules(
            &mut rules,
            MEMORY_MCP_NAME,
            &[
                (ICM_READ_ONLY, McpTier::ReadOnly),
                (ICM_MUTATION, McpTier::Mutation),
                (ICM_DESTRUCTIVE, McpTier::Destructive),
            ],
            icm.mcp_permissions.as_ref(),
        );
    }
    if let Some(cbm) = find_mcp(manifest, CODEBASE_MEMORY_MCP_NAME) {
        push_server_rules(
            &mut rules,
            CODEBASE_MEMORY_MCP_NAME,
            &[
                (CBM_READ_ONLY, McpTier::ReadOnly),
                (CBM_MUTATION, McpTier::Mutation),
                (CBM_DESTRUCTIVE, McpTier::Destructive),
            ],
            cbm.mcp_permissions.as_ref(),
        );
    }
    if manifest.plugins.iter().any(|p| {
        p.marketplace == crate::config::CONTEXT_MODE_MARKETPLACE
            && p.plugin == crate::config::CONTEXT_MODE_PLUGIN
    }) {
        let overrides = manifest
            .capabilities
            .features
            .as_ref()
            .and_then(|f| f.context_mode.as_ref())
            .and_then(|c| c.mcp_permissions.as_ref());
        rules.extend(context_mode_rules(overrides));
    }
    rules
}

fn find_mcp<'a>(manifest: &'a MergedManifest, name: &str) -> Option<&'a ResolvedMcp> {
    manifest.mcps.iter().find(|m| m.name == name)
}

/// The context-mode rules. The code-running tools default to `ask` on opencode, because
/// they run arbitrary code. Claude Code keeps them in the mutation tier.
fn context_mode_rules(overrides: Option<&McpPermissions>) -> Vec<Rule> {
    let mutation: Vec<&str> = CTX_MUTATION
        .iter()
        .copied()
        .filter(|tool| !CTX_EXECUTE.contains(tool))
        .collect();
    let mut rules = Vec::new();
    push_server_rules(
        &mut rules,
        CONTEXT_MODE_SERVER,
        &[
            (CTX_READ_ONLY, McpTier::ReadOnly),
            (mutation.as_slice(), McpTier::Mutation),
            (CTX_DESTRUCTIVE, McpTier::Destructive),
            (CTX_EXECUTE, McpTier::Destructive),
        ],
        overrides,
    );
    rules
}

/// Append one rule per tool, with the effect of its tier, for one MCP server.
fn push_server_rules(
    rules: &mut Vec<Rule>,
    server: &str,
    tiers: &[(&[&str], McpTier)],
    overrides: Option<&McpPermissions>,
) {
    let namespace = mcp_namespace(server);
    for (tools, tier) in tiers {
        let effect = match tier_action(*tier, overrides) {
            McpPermissionAction::Allow => "allow",
            McpPermissionAction::Ask => "ask",
            McpPermissionAction::Deny => "deny",
        };
        rules.extend(tools.iter().map(|tool| Rule {
            action: format!("{namespace}_{tool}"),
            resource: "*".to_owned(),
            effect: effect.to_owned(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::super::permissions::Rule;
    use super::{CODEBASE_MEMORY_MCP_NAME, MEMORY_MCP_NAME, context_mode_rules, tier_rules};
    use crate::adapter::mcp_tiers::{ICM_DESTRUCTIVE, ICM_MUTATION, ICM_READ_ONLY};
    use crate::config::{McpPermissionAction, McpPermissions};
    use crate::mcp::resolve::{ResolvedKind, ResolvedMcp};
    use crate::merge::MergedManifest;

    fn rule(action: &str, effect: &str) -> Rule {
        Rule {
            action: action.to_owned(),
            resource: "*".to_owned(),
            effect: effect.to_owned(),
        }
    }

    fn manifest_with_mcp(name: &str, perms: Option<McpPermissions>) -> MergedManifest {
        let mut manifest = MergedManifest::default();
        manifest.mcps.push(ResolvedMcp {
            always_load: None,
            name: name.into(),
            kind: ResolvedKind::Stdio {
                command: "server".into(),
                args: vec![],
                env: std::collections::BTreeMap::new(),
            },
            headers: std::collections::BTreeMap::new(),
            timeout: None,
            disabled_tools: vec![],
            mcp_permissions: perms,
            memory_hook: None,
        });
        manifest
    }

    #[test]
    fn no_feature_mcp_renders_no_tier_rules() {
        assert!(tier_rules(&MergedManifest::default()).is_empty());
    }

    #[test]
    fn icm_tools_map_to_allow_or_ask_by_tier() {
        let rules = tier_rules(&manifest_with_mcp(MEMORY_MCP_NAME, None));
        for tool in ICM_READ_ONLY {
            assert!(
                rules.contains(&rule(&format!("icm_{tool}"), "allow")),
                "{tool}"
            );
        }
        for tool in ICM_DESTRUCTIVE {
            assert!(
                rules.contains(&rule(&format!("icm_{tool}"), "ask")),
                "{tool}"
            );
        }
    }

    #[test]
    fn every_tier_table_entry_gets_one_rule_under_every_override() {
        let expected = ICM_READ_ONLY.len() + ICM_MUTATION.len() + ICM_DESTRUCTIVE.len();
        let choices = [
            None,
            Some(McpPermissionAction::Allow),
            Some(McpPermissionAction::Ask),
            Some(McpPermissionAction::Deny),
        ];
        for read_only in choices {
            for mutation in choices {
                for destructive in choices {
                    let perms = McpPermissions {
                        read_only,
                        mutation,
                        destructive,
                    };
                    let rules = tier_rules(&manifest_with_mcp(MEMORY_MCP_NAME, Some(perms)));
                    assert_eq!(
                        rules.len(),
                        expected,
                        "{read_only:?} {mutation:?} {destructive:?}"
                    );
                    assert!(
                        rules
                            .iter()
                            .all(|r| ["allow", "ask", "deny"].contains(&r.effect.as_str()))
                    );
                }
            }
        }
    }

    #[test]
    fn tier_override_replaces_the_default_effect() {
        let perms = McpPermissions {
            destructive: Some(McpPermissionAction::Deny),
            ..McpPermissions::default()
        };
        let rules = tier_rules(&manifest_with_mcp(MEMORY_MCP_NAME, Some(perms)));
        assert!(rules.contains(&rule("icm_icm_memory_forget", "deny")));
        assert!(!rules.iter().any(|r| r.effect == "ask"));
    }

    #[test]
    fn codebase_memory_tools_use_their_own_server_prefix() {
        let rules = tier_rules(&manifest_with_mcp(CODEBASE_MEMORY_MCP_NAME, None));
        assert!(rules.contains(&rule("codebase-memory-mcp_delete_project", "ask")));
        assert!(rules.contains(&rule("codebase-memory-mcp_search_graph", "allow")));
    }

    #[test]
    fn context_mode_code_execution_asks_by_default() {
        let rules = context_mode_rules(None);
        assert!(rules.contains(&rule("context-mode_ctx_execute", "ask")));
        assert!(rules.contains(&rule("context-mode_ctx_execute_file", "ask")));
        assert!(rules.contains(&rule("context-mode_ctx_batch_execute", "ask")));
        assert!(rules.contains(&rule("context-mode_ctx_search", "allow")));
        assert!(rules.contains(&rule("context-mode_ctx_index", "allow")));
    }

    #[test]
    fn mutation_allow_does_not_unlock_code_execution() {
        let perms = McpPermissions {
            mutation: Some(McpPermissionAction::Allow),
            ..McpPermissions::default()
        };
        let rules = context_mode_rules(Some(&perms));
        assert!(rules.contains(&rule("context-mode_ctx_execute", "ask")));
        assert!(rules.contains(&rule("context-mode_ctx_index", "allow")));
    }
}
