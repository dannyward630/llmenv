//! Writes the Claude Code LSP plugin from a bundle's LSP definitions.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::json;

use super::LSP_PLUGIN_NAME;
use crate::adapter::skills::create_dir_owner_only;

/// Renders `manifest.capabilities.lsp` into `skills/llmenv-lsp/.claude-plugin/plugin.json`.
/// Returns the relative path written, or `None` if nothing rendered (mirrors how the
/// Crush adapter omits its `lsp` key entirely when every server is disabled/skipped).
///
/// Claude Code's `lspServers` schema requires `extensionToLanguage` (file extension →
/// language id). The neutral `filetypes` field (language ids only, e.g. `"rust"`) can't
/// be reliably converted into that — a language id is often not its own extension
/// (`rust` → `.rs`, `python` → `.py`) — so a server without `extension_to_language` set
/// is skipped for Claude Code with a warning, the same "skip + warn loudly" pattern
/// `CrushAdapter` uses for hooks it can't express, rather than a hard error that would
/// break a bundle shared with an engine (Crush) that renders `filetypes` directly.
/// `root_markers` and `timeout` have no Claude Code equivalent (a single `workspaceFolder`
/// path and a startup-only `startupTimeout` respectively, not per-request) and are left
/// unrendered rather than guessed at.
pub(super) fn write_lsp_plugin(
    out: &Path,
    servers: &[crate::config::LspServer],
) -> anyhow::Result<Option<PathBuf>> {
    let mut lsp_servers = serde_json::Map::new();
    for srv in servers {
        if srv.disabled {
            continue;
        }
        if srv.extension_to_language.is_empty() {
            eprintln!(
                "warning: Claude Code requires an extensionToLanguage map for LSP servers — \
                 skipping '{}' for Claude Code. Add capabilities.lsp[].extension_to_language \
                 (e.g. {{\".rs\": \"rust\"}}) to enable it there.",
                srv.name
            );
            continue;
        }
        let mut entry = serde_json::Map::new();
        entry.insert("command".into(), json!(srv.command));
        if !srv.args.is_empty() {
            entry.insert("args".into(), json!(srv.args));
        }
        if !srv.env.is_empty() {
            entry.insert("env".into(), json!(srv.env));
        }
        entry.insert(
            "extensionToLanguage".into(),
            json!(srv.extension_to_language),
        );
        if let Some(opts) = &srv.init_options {
            let as_json = serde_json::to_value(opts).map_err(|err| {
                anyhow::anyhow!(
                    "LSP server '{}': failed to convert init_options to JSON: {err}",
                    srv.name
                )
            })?;
            entry.insert("initializationOptions".into(), as_json);
        }
        lsp_servers.insert(srv.name.clone(), serde_json::Value::Object(entry));
    }

    if lsp_servers.is_empty() {
        return Ok(None);
    }

    let plugin_dir = out
        .join("skills")
        .join(LSP_PLUGIN_NAME)
        .join(".claude-plugin");
    create_dir_owner_only(&plugin_dir)?;
    let manifest = json!({ "name": LSP_PLUGIN_NAME, "lspServers": lsp_servers });
    let rel_path = PathBuf::from("skills")
        .join(LSP_PLUGIN_NAME)
        .join(".claude-plugin")
        .join("plugin.json");
    crate::paths::write_owner_only_atomic(
        &plugin_dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest)?.as_bytes(),
    )
    .with_context(|| format!("writing {}", rel_path.display()))?;

    Ok(Some(rel_path))
}
