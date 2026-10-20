//! The plan entries that need a yes (Q4 of docs/design/issue-2773-onboarding.md): each
//! permission allow, hook, MCP server, language server, plugin, and memory feature entry.

use std::collections::BTreeSet;

use anyhow::Context as _;
use serde_yaml::{Mapping, Value};

use super::Plan;

/// The capability lists whose entries need a yes (Q4): key, sub key, and the item kind.
const GATED_LISTS: [(&str, Option<&str>, &str); 5] = [
    ("permissions", Some("allow"), "permission allow"),
    ("hooks", None, "hook"),
    ("mcp", None, "MCP server"),
    ("lsp", None, "language server"),
    ("plugins", None, "plugin"),
];

/// Where a gated entry lives, so a no can remove it.
#[derive(Debug, Clone, Copy)]
enum Loc {
    Bundle {
        bundle: usize,
        list: usize,
        index: usize,
    },
    Memory(usize),
    CodebaseMemory(usize),
}

impl Plan {
    fn locs(&self) -> Vec<Loc> {
        let mut locs = Vec::new();
        for (b, bundle) in self.bundles.iter().enumerate() {
            for (l, (key, sub, _)) in GATED_LISTS.iter().enumerate() {
                let count = gated_list(&bundle.capabilities, key, *sub).map_or(0, Vec::len);
                locs.extend((0..count).map(|index| Loc::Bundle {
                    bundle: b,
                    list: l,
                    index,
                }));
            }
        }
        locs.extend((0..self.features.memory.len()).map(Loc::Memory));
        locs.extend((0..self.features.codebase_memory.len()).map(Loc::CodebaseMemory));
        locs
    }

    /// The entries that need a yes, in order: a label and the entry as YAML, in full.
    ///
    /// # Errors
    /// An entry cannot be encoded as YAML.
    pub(crate) fn gated_items(&self) -> anyhow::Result<Vec<(String, String)>> {
        self.locs()
            .into_iter()
            .map(|loc| self.describe(loc))
            .collect()
    }

    fn describe(&self, loc: Loc) -> anyhow::Result<(String, String)> {
        let (label, value) = match loc {
            Loc::Bundle {
                bundle,
                list,
                index,
            } => {
                let b = self.bundles.get(bundle).context("no such bundle")?;
                let (key, sub, kind) = GATED_LISTS.get(list).context("no such list")?;
                let entry = gated_list(&b.capabilities, key, *sub)
                    .and_then(|l| l.get(index))
                    .context("no such entry")?;
                (format!("{kind} in bundle {}", b.name), entry.clone())
            }
            Loc::Memory(i) => {
                let entry = self.features.memory.get(i).context("no such entry")?;
                (
                    "ICM memory server (features.memory)".to_string(),
                    entry.clone(),
                )
            }
            Loc::CodebaseMemory(i) => {
                let entry = self
                    .features
                    .codebase_memory
                    .get(i)
                    .context("no such entry")?;
                (
                    "codebase-memory-mcp server (features.codebase_memory)".to_string(),
                    entry.clone(),
                )
            }
        };
        let text = serde_yaml::to_string(&value).context("encoding a plan entry")?;
        Ok((
            label,
            text.trim_start_matches("---\n").trim_end().to_string(),
        ))
    }

    /// Remove the gated entries at the `declined` positions of [`Self::gated_items`].
    pub(crate) fn remove_items(&mut self, declined: &BTreeSet<usize>) {
        let locs = self.locs();
        // Remove from the end, so an earlier index in the same list stays valid.
        for pos in declined.iter().rev() {
            match locs.get(*pos) {
                Some(Loc::Bundle {
                    bundle,
                    list,
                    index,
                }) => {
                    let Some(b) = self.bundles.get_mut(*bundle) else {
                        continue;
                    };
                    let Some((key, sub, _)) = GATED_LISTS.get(*list) else {
                        continue;
                    };
                    if let Some(entries) = gated_list_mut(&mut b.capabilities, key, *sub)
                        && *index < entries.len()
                    {
                        entries.remove(*index);
                    }
                }
                Some(Loc::Memory(i)) if *i < self.features.memory.len() => {
                    self.features.memory.remove(*i);
                }
                Some(Loc::CodebaseMemory(i)) if *i < self.features.codebase_memory.len() => {
                    self.features.codebase_memory.remove(*i);
                }
                _ => {}
            }
        }
    }
}

fn gated_list<'a>(caps: &'a Mapping, key: &str, sub: Option<&str>) -> Option<&'a Vec<Value>> {
    let value = caps.get(key)?;
    let value = match sub {
        Some(sub) => value.get(sub)?,
        None => value,
    };
    value.as_sequence()
}

fn gated_list_mut<'a>(
    caps: &'a mut Mapping,
    key: &str,
    sub: Option<&str>,
) -> Option<&'a mut Vec<Value>> {
    let value = caps.get_mut(key)?;
    let value = match sub {
        Some(sub) => value.get_mut(sub)?,
        None => value,
    };
    value.as_sequence_mut()
}
