//! The plan entries that need a yes (Q4 of docs/design/issue-2773-onboarding.md): each
//! permission allow, hook, MCP server, language server, plugin, environment variable of a
//! shipped bundle, memory feature entry, and host.

use std::collections::BTreeSet;

use anyhow::{Context as _, ensure};
use serde_yaml::{Mapping, Value};

use super::{Plan, shown};

/// The capability lists whose entries need a yes: key, sub key, and the item kind.
pub(super) const GATED_LISTS: [(&str, Option<&str>, &str); 5] = [
    ("permissions", Some("allow"), "permission allow"),
    ("hooks", None, "hook"),
    ("mcp", None, "MCP server"),
    ("lsp", None, "language server"),
    ("plugins", None, "plugin"),
];

/// Where a gated entry lives, so a no can remove it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Loc {
    Bundle {
        bundle: usize,
        list: usize,
        index: usize,
    },
    /// A plan bundle cannot set `env`, so only a shipped bundle has one. A variable such as
    /// `RUSTC_WRAPPER` breaks a build when its tool is absent, so each one gets a question.
    Env {
        bundle: usize,
        key: Value,
    },
    Memory(usize),
    CodebaseMemory(usize),
    /// A host decides where a memory server's traffic goes.
    Host(Value),
}

/// One gated entry: where it is, its label, and its value.
struct Entry {
    loc: Loc,
    label: String,
    value: Value,
}

impl Plan {
    fn entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        for (b, bundle) in self.bundles.iter().enumerate() {
            for (l, (key, sub, kind)) in GATED_LISTS.iter().enumerate() {
                let list = gated_list(&bundle.capabilities, key, *sub)
                    .cloned()
                    .unwrap_or_default();
                out.extend(list.into_iter().enumerate().map(|(index, value)| Entry {
                    loc: Loc::Bundle {
                        bundle: b,
                        list: l,
                        index,
                    },
                    label: format!("{kind} in bundle {}", bundle.name),
                    value,
                }));
            }
            let env = bundle.capabilities.get("env").and_then(Value::as_mapping);
            out.extend(env.into_iter().flatten().map(|(key, value)| {
                let mut entry = Mapping::new();
                entry.insert(key.clone(), value.clone());
                Entry {
                    loc: Loc::Env {
                        bundle: b,
                        key: key.clone(),
                    },
                    label: format!("environment variable in bundle {}", bundle.name),
                    value: Value::Mapping(entry),
                }
            }));
        }
        out.extend(
            self.features
                .memory
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, value)| Entry {
                    loc: Loc::Memory(i),
                    label: "ICM memory server (features.memory)".to_string(),
                    value,
                }),
        );
        out.extend(
            self.features
                .codebase_memory
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, value)| Entry {
                    loc: Loc::CodebaseMemory(i),
                    label: "codebase-memory-mcp server (features.codebase_memory)".to_string(),
                    value,
                }),
        );
        out.extend(self.host.iter().map(|(key, value)| Entry {
            loc: Loc::Host(key.clone()),
            label: format!("host {}", shown(key.as_str().unwrap_or_default())),
            value: value.clone(),
        }));
        out
    }

    /// The entries that need a yes, in order: a label and the entry as YAML, in full.
    ///
    /// # Errors
    /// An entry cannot be encoded as YAML.
    pub(crate) fn gated_items(&self) -> anyhow::Result<Vec<(String, String)>> {
        self.entries()
            .into_iter()
            .map(|e| {
                let text = serde_yaml::to_string(&e.value).context("encoding a plan entry")?;
                Ok((e.label, shown_block(text.trim_end())))
            })
            .collect()
    }

    /// Remove the gated entries at the `declined` positions of [`Self::gated_items`].
    ///
    /// # Errors
    /// A position does not match an entry of the plan. Nothing is then safe to write, because
    /// an entry that got a no could stay in.
    pub(crate) fn remove_items(&mut self, declined: &BTreeSet<usize>) -> anyhow::Result<()> {
        let entries = self.entries();
        let mut locs = Vec::new();
        for pos in declined {
            let entry = entries
                .get(*pos)
                .with_context(|| format!("declined entry {pos} is not in the plan"))?;
            locs.push(entry.loc.clone());
        }
        // Remove from the end, so an earlier index in the same list stays valid.
        for loc in locs.into_iter().rev() {
            ensure!(
                self.remove(&loc),
                "declined entry {loc:?} no longer matches the plan"
            );
        }
        Ok(())
    }

    fn remove(&mut self, loc: &Loc) -> bool {
        let take =
            |list: &mut Vec<Value>, i: usize| (i < list.len()).then(|| list.remove(i)).is_some();
        match loc {
            Loc::Bundle {
                bundle,
                list,
                index,
            } => {
                let Some((key, sub, _)) = GATED_LISTS.get(*list) else {
                    return false;
                };
                self.bundles
                    .get_mut(*bundle)
                    .and_then(|b| gated_list_mut(&mut b.capabilities, key, *sub))
                    .is_some_and(|entries| take(entries, *index))
            }
            Loc::Env { bundle, key } => self
                .bundles
                .get_mut(*bundle)
                .and_then(|b| b.capabilities.get_mut("env"))
                .and_then(Value::as_mapping_mut)
                .is_some_and(|env| env.remove(key).is_some()),
            Loc::Memory(i) => take(&mut self.features.memory, *i),
            Loc::CodebaseMemory(i) => take(&mut self.features.codebase_memory, *i),
            Loc::Host(key) => self.host.remove(key).is_some(),
        }
    }
}

/// YAML text from the plan, safe to print: each line keeps its layout, and control characters
/// are escaped.
fn shown_block(text: &str) -> String {
    text.lines().map(shown).collect::<Vec<_>>().join("\n")
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

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::{Loc, Plan};

    #[test]
    fn a_position_past_the_end_of_a_list_removes_nothing() {
        let mut plan = Plan::parse("features: {memory: [{server_host: a, port: 1}]}").unwrap();
        assert!(!plan.remove(&Loc::Memory(1)));
        assert!(plan.remove(&Loc::Memory(0)));
        assert!(plan.is_empty());
    }
}
