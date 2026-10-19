//! The launch of the AI engine that runs the setup skill.

use anyhow::{Context as _, Result};

/// Run the setup skill in `engine_id`, with `skill_content` as its prompt.
///
/// # Errors
/// The engine is unsupported, is not in an absolute PATH folder, cannot start, or exits with an
/// error.
pub(super) fn launch_engine(engine_id: &str, skill_content: &str) -> Result<()> {
    match engine_id {
        "claude_code" => {
            let status = std::process::Command::new(crate::path_lookup::resolve_program("claude")?)
                .arg("-p")
                .arg(skill_content)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
                .context("launching claude")?;
            if !status.success() {
                anyhow::bail!("claude exited with status {status}");
            }
        }
        "crush" => {
            use std::io::Write;
            let mut child =
                std::process::Command::new(crate::path_lookup::resolve_program("crush")?)
                    .arg("run")
                    .arg("--quiet")
                    .arg("Execute the setup-llmenv skill")
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                    .context("launching crush")?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(skill_content.as_bytes())
                    .context("writing skill to crush stdin")?;
            }
            let status = child.wait().context("waiting for crush")?;
            if !status.success() {
                anyhow::bail!("crush exited with status {status}");
            }
        }
        other => anyhow::bail!("unsupported engine: {other}"),
    }
    Ok(())
}
