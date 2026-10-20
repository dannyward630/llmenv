//! Shipped bundles in a plan (#2787). The plan names one with `shipped: true`. Its files and
//! capabilities come from the binary, and the gate still asks about each entry.

use std::path::Path;

use anyhow::ensure;

use super::{Plan, PlanBundle, encode_capabilities};
use crate::onboarding::{NewFile, shipped};

impl Plan {
    /// Fill each shipped bundle from the binary: its default tags, when the plan sets none, and
    /// its capabilities, so that the gate asks about each entry.
    pub(super) fn resolve_shipped(&mut self) -> anyhow::Result<()> {
        for bundle in self.bundles.iter_mut().filter(|b| b.shipped) {
            ensure!(
                bundle.agents_md.is_empty() && bundle.capabilities.is_empty(),
                "bundle {:?} is shipped, so its files come from llmenv. Fix: remove `agents_md` \
                 and `capabilities` from it in the plan",
                bundle.name
            );
            let entry = shipped::find(&bundle.name)?;
            if bundle.when.is_empty() {
                bundle.when = entry.when;
            }
            let capabilities = shipped::capabilities(&bundle.name)?;
            bundle.capabilities = capabilities.clone();
            bundle.shipped_capabilities = Some(capabilities);
        }
        Ok(())
    }
}

/// The files of a shipped bundle under `dir`. `bundle.yaml` keeps the shipped text when every
/// entry got a yes. After a no, it holds the entries that remain, and its comments are lost.
pub(super) fn shipped_files(bundle: &PlanBundle, dir: &Path) -> anyhow::Result<Vec<NewFile>> {
    let unchanged = bundle.shipped_capabilities.as_ref() == Some(&bundle.capabilities);
    shipped::files(&bundle.name)
        .map(|file| {
            let text = if file.path == "bundle.yaml" && !unchanged {
                format!(
                    "# The shipped llmenv bundle {}, without the entries that got a no in \
                     `llmenv setup --apply`.\n{}",
                    bundle.name,
                    encode_capabilities(bundle)?
                )
            } else {
                file.text.to_string()
            };
            Ok(NewFile {
                rel: dir.join(file.path),
                text,
                executable: file.executable,
            })
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod tests;
