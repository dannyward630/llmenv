//! The doctor check of the programs that the config needs on PATH.

use crate::config::Config;

use super::has_remote_memory_host;

pub(super) fn run_doctor_tool_availability(use_color: bool, config: &Config) {
    let pass = super::super::doctor_pass(use_color);
    let fail = super::super::doctor_fail(use_color);
    let info = super::super::doctor_info(use_color);

    eprintln!();
    eprintln!("Tool-availability checks:");
    let relative = crate::path_lookup::relative_entries(std::env::var_os("PATH").as_deref());
    if !relative.is_empty() {
        let list: Vec<String> = relative.iter().map(|d| d.display().to_string()).collect();
        eprintln!(
            "{info} PATH holds relative folders ({}). llmenv skips them when it looks for a \
             program, so a program that only they hold reads as not found.",
            list.join(", ")
        );
    }

    let has_memory = config
        .features
        .as_ref()
        .is_some_and(|f| !f.memory.is_empty());

    // icm — required when features.memory has entries
    // mcp-proxy or uvx — required when any memory server_host is remote
    if has_memory {
        if crate::adapter::binary_on_path("icm") {
            eprintln!("{pass} icm found on PATH");
        } else {
            eprintln!("{fail} icm not found on PATH (required when features.memory is configured)");
        }

        if has_remote_memory_host(config) {
            if crate::adapter::binary_on_path("mcp-proxy") || crate::adapter::binary_on_path("uvx")
            {
                eprintln!("{pass} mcp-proxy or uvx found on PATH (remote memory server_host)");
            } else {
                eprintln!(
                    "{fail} neither mcp-proxy nor uvx on PATH \
                     (remote memory server_host requires one for TCP proxying)"
                );
            }
        }
    }

    // codebase-memory-mcp — required when features.codebase_memory has entries
    let has_codebase_memory = config
        .features
        .as_ref()
        .is_some_and(|f| !f.codebase_memory.is_empty());
    if has_codebase_memory {
        if crate::adapter::binary_on_path("codebase-memory-mcp") {
            eprintln!("{pass} codebase-memory-mcp found on PATH");
        } else {
            eprintln!(
                "{fail} codebase-memory-mcp not found on PATH (required when \
                 features.codebase_memory is configured) — install: \
                 https://github.com/DeusData/codebase-memory-mcp"
            );
        }
    }

    // claude — required when claude_code engine is not disabled
    let claude_disabled = config
        .disabled_engines
        .iter()
        .any(|e| e.eq_ignore_ascii_case("claude_code"));
    if !claude_disabled {
        if crate::adapter::binary_on_path("claude") {
            eprintln!("{pass} claude found on PATH");
        } else {
            eprintln!(
                "{fail} claude not found on PATH \
                 (claude_code engine is not disabled, but the `claude` binary is missing)"
            );
        }
    }

    // Every other registered engine — always optional. Derived from the adapter
    // registry rather than a hardcoded list, so a newly registered engine is
    // reported here without a second edit (#1032).
    for adapter in crate::adapter::registered_adapters() {
        if crate::adapter::engine_id(adapter.as_ref()) == "claude_code" {
            continue; // reported above, where it is required rather than optional
        }
        let bin = adapter.binary_name();
        if crate::adapter::binary_on_path(bin) {
            eprintln!("{pass} {bin} found on PATH");
        } else {
            eprintln!("{info} {bin} not found on PATH (optional engine)");
        }
    }
}
