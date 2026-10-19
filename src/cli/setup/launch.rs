//! The launch of the AI engine that runs the setup skill.

use anyhow::{Context as _, Result};

/// Run the setup skill in `engine_id`, with `skill_content` as its prompt.
///
/// Both arms go through `run_supervised` (#1385): this hands the terminal to an interactive agent
/// session that can run for minutes, and an unsupervised wait let a Ctrl-C kill `llmenv setup`
/// while the agent kept running.
///
/// # Errors
/// The engine is unsupported, is not in an absolute PATH folder, cannot start, or exits with an
/// error.
pub(super) fn launch_engine(engine_id: &str, skill_content: &str) -> Result<()> {
    match engine_id {
        "claude_code" => {
            let mut cmd = crate::cli::command_for_binary("claude")?;
            cmd.arg("-p")
                .arg(skill_content)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());
            let status =
                crate::cli::run_supervised(cmd, "claude", None).context("launching claude")?;
            if !status.success() {
                anyhow::bail!("claude exited with status {status}");
            }
        }
        "crush" => {
            let mut cmd = crate::cli::command_for_binary("crush")?;
            cmd.arg("run")
                .arg("--quiet")
                .arg("Execute the setup-llmenv skill")
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());
            // crush takes the skill text on stdin; `run_supervised` pipes it,
            // writes it, and closes the pipe so crush sees EOF.
            let status = crate::cli::run_supervised(cmd, "crush", Some(skill_content.as_bytes()))
                .context("launching crush")?;
            if !status.success() {
                anyhow::bail!("crush exited with status {status}");
            }
        }
        other => anyhow::bail!("unsupported engine: {other}"),
    }
    Ok(())
}
