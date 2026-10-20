//! The flags of `llmenv setup`, and the dispatch between the onboarding facts and the wizard.

use std::path::PathBuf;

use anyhow::{Context as _, Result};

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
    /// Print the bundles that ship in llmenv as JSON: each name, default tags, purpose, and
    /// files. An onboarding plan adds one with `shipped: true`
    #[arg(long, conflicts_with_all = ["path", "repo", "rescan", "no_launch", "scan", "tools"])]
    bundles: bool,
    /// Apply an onboarding plan: check it, ask for a yes for the plan and for each hook, MCP
    /// server, and permission allow, then create the absent files and add to config.yaml.
    /// Needs a terminal
    #[arg(
        long,
        value_name = "PLAN",
        conflicts_with_all = ["path", "repo", "rescan", "no_launch", "scan", "tools", "bundles"]
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

/// Run `llmenv setup --apply PLAN`, then validate and regenerate when it wrote files. The plan
/// text goes to stderr, next to the prompts, so a redirected stdout cannot hide what the user
/// approves.
///
/// # Errors
/// stdin or stderr is not a terminal, the apply fails, or validate or regenerate fails.
fn run_apply(plan: &std::path::Path) -> Result<()> {
    use std::io::IsTerminal as _;
    // The agent's shell has no terminal, so only the user can give these answers (#2788).
    anyhow::ensure!(
        std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
        "`llmenv setup --apply` asks for a yes for each hook, MCP server, and permission allow, \
         so it needs a terminal. Fix: run `llmenv setup --apply {}` yourself in a terminal \
         window",
        plan.display()
    );
    let config_dir = crate::paths::config_dir()?;
    let mut out = std::io::stderr();
    if !crate::onboarding::apply::apply(plan, &config_dir, &mut TerminalPrompter, &mut out)? {
        return Ok(());
    }
    let written = "`llmenv setup --apply` wrote the bundle files and config.yaml shown above";
    super::super::run_validate(std::io::stdout().is_terminal()).with_context(|| {
        format!("{written}, but validate failed. Fix: remove the added lines, or fix them by hand")
    })?;
    super::super::run_regenerate().with_context(|| {
        format!(
            "{written}, but regenerate failed. Fix: correct the error, then run `llmenv regenerate`"
        )
    })
}

/// Run `llmenv setup`: a scan, a tool report, or the shipped bundle list prints its JSON and
/// stops; otherwise the wizard runs.
///
/// # Errors
/// The scan, the tool report, the bundle list, or the wizard fails. See
/// [`crate::onboarding::scan::scan_json`], [`crate::onboarding::tools::report_json`],
/// [`crate::onboarding::shipped::listing_json`], and [`run_setup`](super::run_setup).
pub(in crate::cli) fn run_setup_command(args: SetupArgs) -> Result<()> {
    if args.tools {
        println!("{}", crate::onboarding::tools::report_json()?);
        return Ok(());
    }
    if args.bundles {
        println!("{}", crate::onboarding::shipped::listing_json()?);
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
