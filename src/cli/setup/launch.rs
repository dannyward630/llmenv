//! The launch of the AI engine that runs the setup skill.

use std::process::{Command, ExitStatus};

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
            let cmd = claude_command(crate::cli::command_for_binary("claude")?, skill_content);
            let status =
                crate::cli::run_supervised(cmd, "claude", None).context("launching claude")?;
            ensure_success("claude", status)
        }
        "crush" => {
            let cmd = crush_command(crate::cli::command_for_binary("crush")?);
            // crush takes the skill text on stdin; `run_supervised` pipes it,
            // writes it, and closes the pipe so crush sees EOF.
            let status = crate::cli::run_supervised(cmd, "crush", Some(skill_content.as_bytes()))
                .context("launching crush")?;
            ensure_success("crush", status)
        }
        other => anyhow::bail!("unsupported engine: {other}"),
    }
}

/// `claude -p <skill>`, with the terminal handed to the session.
fn claude_command(mut cmd: Command, skill_content: &str) -> Command {
    cmd.arg("-p")
        .arg(skill_content)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    cmd
}

/// `crush run --quiet <prompt>`. The skill text arrives on stdin, not as an argument.
fn crush_command(mut cmd: Command) -> Command {
    cmd.arg("run")
        .arg("--quiet")
        .arg("Execute the setup-llmenv skill")
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    cmd
}

fn ensure_success(engine: &str, status: ExitStatus) -> Result<()> {
    if !status.success() {
        anyhow::bail!("{engine} exited with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
