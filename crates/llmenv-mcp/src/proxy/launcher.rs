//! The program that runs the memory proxy: `mcp-proxy`, or `uvx mcp-proxy`.

use std::path::PathBuf;

/// Builds the `mcp-proxy` invocation, preferring a `mcp-proxy` already on
/// `PATH` and falling back to `uvx mcp-proxy` when it isn't installed. Returns
/// the program plus its leading args; the caller appends `--port`/target.
///
/// # Errors
/// Returns an error when neither `mcp-proxy` nor `uvx` is on `PATH` — the
/// memory backend can't be exposed on the network without one of them.
pub(super) fn mcp_proxy_command() -> anyhow::Result<(PathBuf, Vec<&'static str>)> {
    mcp_proxy_command_in(std::env::var_os("PATH").as_deref())
}

/// [`mcp_proxy_command`] with `PATH` passed in. The program is the absolute path that the lookup
/// found, so the spawn cannot pick another file of that name from a relative `PATH` folder (#2932).
pub(super) fn mcp_proxy_command_in(
    path_var: Option<&std::ffi::OsStr>,
) -> anyhow::Result<(PathBuf, Vec<&'static str>)> {
    if let Some(program) = llmenv_paths::program::find_on_path("mcp-proxy", path_var) {
        Ok((program, vec![]))
    } else if let Some(program) = llmenv_paths::program::find_on_path("uvx", path_var) {
        Ok((program, vec!["mcp-proxy"]))
    } else {
        Err(anyhow::anyhow!(
            "neither `mcp-proxy` nor `uvx` found in an absolute PATH folder (relative folders \
             such as `.` are skipped); install one to run the memory server, or disable the \
             `memory` config block"
        ))
    }
}
