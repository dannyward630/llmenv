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
    if !args.scan.is_empty() {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        println!(
            "{}",
            crate::onboarding::scan::scan_json(&args.scan, home.as_deref())?
        );
        return Ok(());
    }
    run_setup(args.path, args.repo, args.no_launch, args.rescan)
}
