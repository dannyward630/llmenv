//! The dependent-tool version report at the end of `llmenv doctor`.

use super::version::{cbm_floor_warning, opencode_floor_warning};
use super::{DEPENDENT_TOOLS, UpdatePath, tool_version};

pub(super) fn run_doctor_dependent_tools(use_color: bool) {
    let pass = super::super::doctor_pass(use_color);
    let info = super::super::doctor_info(use_color);
    let warn = super::super::doctor_warning(use_color);

    let installed: Vec<_> = DEPENDENT_TOOLS
        .iter()
        .filter(|(bin, _)| crate::adapter::binary_on_path(bin))
        .collect();
    if installed.is_empty() {
        return;
    }

    eprintln!();
    eprintln!("Dependent-tool versions:");
    for (bin, update) in installed {
        let version = tool_version(bin).unwrap_or_else(|| "version unknown".to_string());
        let how = match update {
            UpdatePath::SelfApply(_) => "update with",
            UpdatePath::Reports(_) => "check for updates with",
        };
        let floor_warning = match *bin {
            "codebase-memory-mcp" => cbm_floor_warning(&version),
            "opencode" => opencode_floor_warning(&version),
            _ => None,
        };
        let marker = if version == "version unknown" {
            &info
        } else if floor_warning.is_some() {
            &warn
        } else {
            &pass
        };
        eprintln!("{marker} {bin} {version} — {how} `{}`", update.command());
        if let Some(text) = floor_warning {
            eprintln!("{warn} {text}");
        }
    }
}
