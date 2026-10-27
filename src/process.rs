//! Child processes with a deadline. The helper lives in `llmenv_mcp`, so the MCP crate and the
//! root crate share one copy.

pub(crate) use llmenv_mcp::process::run_traced;

#[cfg(test)]
mod tests {
    // A hot-path spawn must go through `run_traced`, so each one prints a
    // `[LLMENV_SPAWN]` line (#3010). A raw wait here would skip the trace.
    #[test]
    fn hot_path_files_wait_on_children_only_through_run_traced() {
        let files = [
            (
                "scope/probe.rs",
                include_str!("../crates/llmenv-scope/src/probe.rs"),
            ),
            ("git.rs", include_str!("git.rs")),
            (
                "cli/statusline/widget.rs",
                include_str!("cli/statusline/widget.rs"),
            ),
            (
                "auth/credentials/keychain.rs",
                include_str!("auth/credentials/keychain.rs"),
            ),
            (
                "materialize/open_files.rs",
                include_str!("materialize/open_files.rs"),
            ),
            (
                "task/session/checkpoint.rs",
                include_str!("../crates/llmenv-task/src/session/checkpoint.rs"),
            ),
            (
                "adapter/claude_code/version.rs",
                include_str!("adapter/claude_code/version.rs"),
            ),
            (
                "mcp/cbm_roots.rs",
                include_str!("../crates/llmenv-mcp/src/cbm_roots.rs"),
            ),
            ("cli/doctor.rs", include_str!("cli/doctor.rs")),
            (
                "cli/upgrade/install.rs",
                include_str!("cli/upgrade/install.rs"),
            ),
        ];
        for (name, text) in files {
            let code = text.split("#[cfg(test)]").next().unwrap_or(text);
            for raw in ["run_with_timeout(", ".spawn()", ".output()"] {
                assert!(
                    !code.contains(raw),
                    "{name} calls `{raw}`; use process::run_traced"
                );
            }
        }
    }
}
