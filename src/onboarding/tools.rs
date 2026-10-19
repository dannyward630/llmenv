//! The candidate tools for onboarding, and which of them are on `PATH` (#2791). The report makes
//! no network call and installs nothing; the onboarding skill decides what to suggest. Design:
//! docs/design/issue-2773-onboarding.md ("Tool suggestions").

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// The candidate list. It is data, so a new tool needs no code change.
const CANDIDATES: &str = include_str!("tools.yaml");

/// One candidate tool, as `tools.yaml` describes it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Tool {
    name: String,
    purpose: String,
    evidence: String,
    #[serde(default)]
    signals: Vec<String>,
    binaries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    feature: Option<String>,
    #[serde(default)]
    strong_default: bool,
    install: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidates {
    tools: Vec<Tool>,
}

/// One tool with the binary found on `PATH`, if any.
#[derive(Debug, Serialize)]
struct ToolStatus {
    #[serde(flatten)]
    tool: Tool,
    found: Option<PathBuf>,
}

/// The tool report, printed as JSON.
#[derive(Debug, Serialize)]
struct ToolReport {
    tools: Vec<ToolStatus>,
}

fn candidates() -> anyhow::Result<Vec<Tool>> {
    let parsed: Candidates = serde_yaml::from_str(CANDIDATES)
        .context("parsing the built-in onboarding tool list (src/onboarding/tools.yaml)")?;
    Ok(parsed.tools)
}

fn report(path_var: Option<&OsStr>) -> anyhow::Result<ToolReport> {
    let tools = candidates()?
        .into_iter()
        .map(|tool| {
            let found = tool
                .binaries
                .iter()
                .find_map(|binary| find_on_path(binary, path_var));
            ToolStatus { tool, found }
        })
        .collect();
    Ok(ToolReport { tools })
}

/// Report every candidate tool with the binary found on the current `PATH`, as pretty JSON.
///
/// # Errors
/// The built-in tool list does not parse, which is a build defect that a test catches.
pub(crate) fn report_json() -> anyhow::Result<String> {
    let path_var = std::env::var_os("PATH");
    let report = report(path_var.as_deref())?;
    serde_json::to_string_pretty(&report).context("serializing the tool report")
}

/// The first executable file named `name` in the folders of `path_var`. A name that holds a path
/// separator is not looked up, so the data file cannot point at an arbitrary path.
fn find_on_path(name: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    if name.is_empty() || name.contains(std::path::is_separator) {
        return None;
    }
    std::env::split_paths(path_var?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}
