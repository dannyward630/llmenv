//! opencode 2.x tool names and argument keys, mapped to Claude Code's for the hook payload
//! (#2841).
//!
//! Hook handlers and bundle hook scripts read Claude Code tool names and argument keys. The
//! opencode plugin renames each call with this table before it runs a hook, so every hook sees
//! one payload shape. The tool names are the `name` exports of opencode's
//! `packages/core/src/tool/plugin/*.ts` at v2.0.31.

/// One opencode tool: its name in a hook event, the Claude Code name, and the argument keys
/// that differ.
#[derive(Debug)]
pub(crate) struct HookTool {
    opencode: &'static str,
    claude: &'static str,
    args: &'static [(&'static str, &'static str)],
}

/// The opencode tools that have a Claude Code equivalent. `patch` has none: its one argument is
/// a patch text, not a file path, so it keeps its own name.
const HOOK_TOOLS: &[HookTool] = &[
    HookTool {
        opencode: "shell",
        claude: "Bash",
        args: &[("background", "run_in_background")],
    },
    HookTool {
        opencode: "read",
        claude: "Read",
        args: &[("path", "file_path")],
    },
    HookTool {
        opencode: "edit",
        claude: "Edit",
        args: &[
            ("path", "file_path"),
            ("oldString", "old_string"),
            ("newString", "new_string"),
            ("replaceAll", "replace_all"),
        ],
    },
    HookTool {
        opencode: "write",
        claude: "Write",
        args: &[("path", "file_path")],
    },
    HookTool {
        opencode: "glob",
        claude: "Glob",
        args: &[],
    },
    HookTool {
        opencode: "grep",
        claude: "Grep",
        args: &[],
    },
    HookTool {
        opencode: "webfetch",
        claude: "WebFetch",
        args: &[],
    },
    HookTool {
        opencode: "websearch",
        claude: "WebSearch",
        args: &[],
    },
    HookTool {
        opencode: "subagent",
        claude: "Agent",
        args: &[("agent", "subagent_type")],
    },
    HookTool {
        opencode: "skill",
        claude: "Skill",
        args: &[],
    },
    HookTool {
        opencode: "question",
        claude: "AskUserQuestion",
        args: &[],
    },
];

/// The `tools` object of `llmenv-hooks.json`: opencode name to `{ name, args }`.
pub(crate) fn tools_json() -> serde_json::Value {
    HOOK_TOOLS
        .iter()
        .map(|t| {
            let args: serde_json::Map<String, serde_json::Value> = t
                .args
                .iter()
                .map(|(from, to)| ((*from).to_string(), serde_json::Value::from(*to)))
                .collect();
            (
                t.opencode.to_string(),
                serde_json::json!({ "name": t.claude, "args": args }),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// The namespace that opencode gives the tools of the MCP server `server`: each UTF-16 unit
/// outside `[a-zA-Z0-9_-]` becomes `_`, as the JavaScript `replace` in
/// `packages/core/src/tool/mcp.ts` does. A tool is then named `<namespace>_<tool>`.
pub(crate) fn mcp_namespace(server: &str) -> String {
    let mut out = String::with_capacity(server.len());
    for c in server.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.extend(std::iter::repeat_n('_', c.len_utf16()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_opencode_name_and_claude_name_is_unique() {
        let mut opencode: Vec<_> = HOOK_TOOLS.iter().map(|t| t.opencode).collect();
        let mut claude: Vec<_> = HOOK_TOOLS.iter().map(|t| t.claude).collect();
        let total = HOOK_TOOLS.len();
        opencode.sort_unstable();
        opencode.dedup();
        claude.sort_unstable();
        claude.dedup();
        assert_eq!((opencode.len(), claude.len()), (total, total));
    }

    #[test]
    fn the_file_tools_name_the_path_file_path() {
        let json = tools_json();
        for tool in ["read", "edit", "write"] {
            assert_eq!(json[tool]["args"]["path"], "file_path", "{tool}");
        }
        assert_eq!(json["shell"]["name"], "Bash");
        assert_eq!(json["edit"]["args"]["oldString"], "old_string");
    }

    #[test]
    fn an_mcp_namespace_keeps_only_safe_characters() {
        assert_eq!(mcp_namespace("codebase-memory-mcp"), "codebase-memory-mcp");
        assert_eq!(mcp_namespace("my.server/x"), "my_server_x");
        // A character outside the BMP is two UTF-16 units, so two underscores.
        assert_eq!(mcp_namespace("a\u{1F600}b"), "a__b");
    }
}
