//! `llmenv setup --apply PLAN` (#2788, #2789): check an onboarding plan, ask for a yes for the
//! plan and for each gated entry, then create the absent files and add to `config.yaml`.
//! Design: docs/design/issue-2773-onboarding.md ("The plan file and `llmenv setup --apply`").

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail, ensure};

use super::config_splice::splice;
use super::plan::Plan;
use crate::config::Config;
use crate::merge::BundleRef;

/// The largest config.yaml that `--apply` reads, as for every other read of the config.
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

/// Asks the user a yes-or-no question. The terminal asks in production; a test answers.
pub(crate) trait Prompter {
    /// # Errors
    /// The answer cannot be read.
    fn confirm(&mut self, question: &str) -> anyhow::Result<bool>;
}

/// Check the plan at `plan_path` against the config in `config_dir`, ask for the yes answers,
/// and write. Returns true when it wrote files. A no to the plan or to the `config.yaml`
/// change writes nothing.
///
/// # Errors
/// The plan or the config cannot be read, the plan fails a check, a file to create exists, the
/// config changed during the run, or a write fails.
pub(crate) fn apply(
    plan_path: &Path,
    config_dir: &Path,
    prompter: &mut dyn Prompter,
    out: &mut dyn Write,
) -> anyhow::Result<bool> {
    let config_path = config_dir.join("config.yaml");
    let old = read_config(&config_path)?;
    let config = Config::load(&config_path)?;
    let mut plan = Plan::load(plan_path)?;
    let existing: BTreeSet<String> = config.bundle.iter().map(|b| b.name.clone()).collect();
    plan.skip_existing(&existing, &config_dir.join("bundles"));
    check_hosts(&plan, &config)?;
    for name in plan.skipped() {
        writeln!(
            out,
            "Skipped bundle {name}: it exists, and llmenv never replaces a bundle."
        )?;
    }
    if plan.is_empty() {
        writeln!(out, "The plan adds nothing. Nothing was written.")?;
        return Ok(false);
    }
    check(&plan, &old, config_dir)?;
    print_plan(&plan, out)?;
    if !prompter.confirm("Apply this plan?")? {
        writeln!(out, "Nothing was written.")?;
        return Ok(false);
    }
    ask_items(&mut plan, prompter, out)?;
    check(&plan, &old, config_dir)?;
    let (new_text, diff) = splice(&old, &plan.additions())?;
    writeln!(out, "\nLines to add to {}:\n{diff}", config_path.display())?;
    if !prompter.confirm("Add these lines to config.yaml?")? {
        writeln!(
            out,
            "Nothing was written: a bundle that config.yaml does not list never fires."
        )?;
        return Ok(false);
    }
    write_all(&plan, config_dir, &old, &new_text, out)?;
    Ok(true)
}

fn read_config(path: &Path) -> anyhow::Result<String> {
    llmenv_paths::read_text_bounded(path, MAX_CONFIG_BYTES)
        .with_context(|| format!("reading {}", path.display()))?
        .with_context(|| {
            format!(
                "{} does not exist. Fix: run `llmenv setup` first, then apply the plan",
                path.display()
            )
        })
}

fn check_hosts(plan: &Plan, config: &Config) -> anyhow::Result<()> {
    for name in plan.host_names() {
        ensure!(
            !config.host.contains_key(&name),
            "the plan adds host {name:?}, which config.yaml already has. Fix: use the existing \
             host in the plan's features entry, and drop it from the plan's `host`"
        );
    }
    Ok(())
}

/// The changed config must load, and the merge of every bundle, old and new, must succeed.
/// Every bundle counts as firing, so a scalar collision between any two bundles shows now.
fn check(plan: &Plan, old: &str, config_dir: &Path) -> anyhow::Result<()> {
    let (new_text, _) = splice(old, &plan.additions())?;
    let tmp = tempfile::tempdir().context("making a folder to check the plan")?;
    std::fs::write(tmp.path().join("config.yaml"), &new_text).context("writing the check copy")?;
    for (rel, text) in plan.files()? {
        let path = tmp.path().join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("making the check copy")?;
        }
        std::fs::write(&path, text).context("writing the check copy")?;
    }
    let config = Config::load(&tmp.path().join("config.yaml"))
        .context("the config with the plan added does not load")?;
    let new: BTreeSet<String> = plan.bundle_names().into_iter().collect();
    let refs: Vec<BundleRef> = config
        .bundle
        .iter()
        .map(|b| {
            let root = if new.contains(&b.name) {
                tmp.path()
            } else {
                config_dir
            };
            BundleRef {
                name: b.name.clone(),
                path: root.join("bundles").join(&b.name),
                precedence: 1,
            }
        })
        .filter(|r| r.path.is_dir())
        .collect();
    crate::merge::merge(&config.capabilities, &config.native, &refs)
        .context("the bundles do not merge with the plan added")?;
    Ok(())
}

fn print_plan(plan: &Plan, out: &mut dyn Write) -> anyhow::Result<()> {
    writeln!(out, "The plan adds:")?;
    for name in plan.bundle_names() {
        writeln!(out, "  bundle {name}")?;
    }
    for (rel, _) in plan.files()? {
        writeln!(out, "  file {}", rel.display())?;
    }
    for name in plan.host_names() {
        writeln!(out, "  host {name}")?;
    }
    let items = plan.gated_items()?.len();
    writeln!(
        out,
        "{items} entr{} need a separate yes.",
        if items == 1 { "y" } else { "ies" }
    )?;
    Ok(())
}

/// Ask for each gated entry, and remove each one that gets a no.
fn ask_items(
    plan: &mut Plan,
    prompter: &mut dyn Prompter,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let mut declined = BTreeSet::new();
    for (pos, (label, text)) in plan.gated_items()?.into_iter().enumerate() {
        writeln!(out, "\n{label}:")?;
        for line in text.lines() {
            writeln!(out, "    {line}")?;
        }
        if !prompter.confirm(&format!("Allow this {label}?"))? {
            declined.insert(pos);
        }
    }
    if !declined.is_empty() {
        writeln!(
            out,
            "\n{} entr{} left out.",
            declined.len(),
            if declined.len() == 1 {
                "y is"
            } else {
                "ies are"
            }
        )?;
    }
    plan.remove_items(&declined);
    Ok(())
}

/// Create the files, then write `config.yaml` last, so a failed file write leaves the config as
/// it was. Every path is checked first, so no file is half-written next to one that exists.
fn write_all(
    plan: &Plan,
    config_dir: &Path,
    old: &str,
    new_text: &str,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let files: Vec<(PathBuf, String)> = plan
        .files()?
        .into_iter()
        .map(|(rel, text)| (config_dir.join(rel), text))
        .collect();
    for (path, _) in &files {
        if path.symlink_metadata().is_ok() {
            bail!(
                "{} exists, and llmenv never overwrites a file. Nothing was written",
                path.display()
            );
        }
    }
    for (path, text) in &files {
        create_new(path, text)?;
        writeln!(out, "Created {}", path.display())?;
    }
    let config_path = config_dir.join("config.yaml");
    ensure!(
        read_config(&config_path)? == old,
        "{} changed while the plan ran, so it was not written. The bundle files are created. \
         Fix: run `llmenv setup --apply` again",
        config_path.display()
    );
    llmenv_paths::write_user_file_atomic(&config_path, new_text.as_bytes())
        .with_context(|| format!("writing {}", config_path.display()))?;
    writeln!(out, "Updated {}", config_path.display())?;
    Ok(())
}

fn create_new(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("making {}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests;
