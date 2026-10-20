//! The flags of `llmenv setup`, and the dispatch between the onboarding facts and the wizard.

use std::path::PathBuf;

use anyhow::Result;

use super::run_setup;

/// The flags of `llmenv setup`.
#[derive(Debug, clap::Args)]
pub(in crate::cli) struct SetupArgs {
    /// Directory to set up (defaults to the standard config dir)
    path: Option<PathBuf>,
    /// Repository to store config in (optional)
    #[arg(long)]
    repo: Option<String>,
    /// Skip the AI engine handoff at the end
    #[arg(long)]
    no_launch: bool,
    /// Re-scan existing configs without overwriting config files
    #[arg(long)]
    rescan: bool,
    /// Scan these folders for repositories and print the evidence as JSON, for onboarding.
    /// Reads file names and manifest keys only, and writes nothing
    #[arg(
        long,
        value_name = "ROOT",
        num_args = 1..,
        conflicts_with_all = ["path", "repo", "rescan", "no_launch"]
    )]
    scan: Vec<PathBuf>,
    /// Print the candidate tools for onboarding as JSON, each with the binary found on PATH.
    /// Makes no network call and installs nothing
    #[arg(long, conflicts_with_all = ["path", "repo", "rescan", "no_launch", "scan"])]
    tools: bool,
    /// Apply an onboarding plan: check it, ask for a yes for the plan and for each hook, MCP
    /// server, and permission allow, then create the absent files and add to config.yaml.
    /// Needs a terminal
    #[arg(
        long,
        value_name = "PLAN",
        conflicts_with_all = ["path", "repo", "rescan", "no_launch", "scan", "tools"]
    )]
    apply: Option<PathBuf>,
}

/// Asks on the terminal, with no as the default answer.
struct TerminalPrompter;

impl crate::onboarding::apply::Prompter for TerminalPrompter {
    fn confirm(&mut self, question: &str) -> Result<bool> {
        dialoguer::Confirm::new()
            .with_prompt(question)
            .default(false)
            .interact()
            .map_err(|e| anyhow::anyhow!("reading the answer to {question:?}: {e}"))
    }
}

/// Run `llmenv setup --apply PLAN`, then validate and regenerate when it wrote files.
///
/// # Errors
/// stdin or stderr is not a terminal, the apply fails, or validate or regenerate fails.
fn run_apply(plan: &std::path::Path) -> Result<()> {
    use std::io::IsTerminal as _;
    // The agent's shell has no terminal, so only the user can give these answers (#2788).
    anyhow::ensure!(
        std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
        "`llmenv setup --apply` asks for a yes for each hook, MCP server, and permission allow, \
         so it needs a terminal. Fix: run it yourself in a terminal. In Claude Code, type \
         `! llmenv setup --apply {}`",
        plan.display()
    );
    let config_dir = crate::paths::config_dir()?;
    let mut out = std::io::stdout();
    if crate::onboarding::apply::apply(plan, &config_dir, &mut TerminalPrompter, &mut out)? {
        super::super::run_validate(out.is_terminal())?;
        super::super::run_regenerate()?;
    }
    Ok(())
}

/// Run `llmenv setup`: a scan or a tool report prints its JSON and stops; otherwise the wizard
/// runs.
///
/// # Errors
/// The scan, the tool report, or the wizard fails. See [`crate::onboarding::scan::scan_json`],
/// [`crate::onboarding::tools::report_json`], and [`run_setup`](super::run_setup).
pub(in crate::cli) fn run_setup_command(args: SetupArgs) -> Result<()> {
    if args.tools {
        println!("{}", crate::onboarding::tools::report_json()?);
        return Ok(());
    }
    if let Some(plan) = &args.apply {
        return run_apply(plan);
    }
    if !args.scan.is_empty() {
        println!("{}", crate::onboarding::scan::scan_json(&args.scan)?);
        return Ok(());
    }
    run_setup(args.path, args.repo, args.no_launch, args.rescan)
}
