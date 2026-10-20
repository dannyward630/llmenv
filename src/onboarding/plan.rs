//! The onboarding plan that `llmenv setup --apply` reads (#2788). The skill writes it; this
//! module parses it, checks its shape, lists the entries that need a yes, and gives the files
//! and the `config.yaml` additions to write.
//! Design: docs/design/issue-2773-onboarding.md ("The plan file and `llmenv setup --apply`").

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use super::NewFile;
use super::config_splice::Addition;

/// A plan holds rules text and a few entries. 1 MiB is far above any real plan, and it stops a
/// runaway file before it fills memory.
const MAX_PLAN_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanBundle {
    name: String,
    #[serde(default)]
    when: Vec<String>,
    /// The bundle's rules text. It is the only file a plan writes besides `bundle.yaml`.
    #[serde(default)]
    agents_md: String,
    #[serde(default)]
    capabilities: Mapping,
    /// The files come from the shipped bundle of this name (#2787), not from the plan.
    #[serde(default)]
    shipped: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanFeatures {
    #[serde(default)]
    memory: Vec<Value>,
    #[serde(default)]
    codebase_memory: Vec<Value>,
}

/// New scope entries. A scope only emits tags, so it needs no yes.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanScopes {
    #[serde(default)]
    network: Vec<Value>,
    #[serde(default)]
    host: Vec<Value>,
    #[serde(default)]
    user: Vec<Value>,
    #[serde(default)]
    content: Vec<Value>,
}

impl PlanScopes {
    fn kinds(&self) -> [(&'static str, &Vec<Value>); 4] {
        [
            ("network", &self.network),
            ("host", &self.host),
            ("user", &self.user),
            ("content", &self.content),
        ]
    }
}

/// An onboarding plan.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Plan {
    #[serde(default)]
    bundles: Vec<PlanBundle>,
    #[serde(default)]
    features: PlanFeatures,
    #[serde(default)]
    host: Mapping,
    #[serde(default)]
    scope: PlanScopes,
}

impl Plan {
    /// Read and check the plan at `path`.
    ///
    /// # Errors
    /// The file cannot be read, is not a valid plan, or breaks a shape rule. See
    /// docs/design/issue-2773-onboarding.md ("Plan shape").
    pub(crate) fn load(path: &Path) -> anyhow::Result<Self> {
        let text = llmenv_paths::read_text_bounded(path, MAX_PLAN_BYTES)
            .with_context(|| format!("reading the plan {}", path.display()))?
            .with_context(|| format!("the plan {} does not exist", path.display()))?;
        Self::parse(&text).with_context(|| format!("the plan {} is not valid", path.display()))
    }

    fn parse(text: &str) -> anyhow::Result<Self> {
        let raw: Value = serde_yaml::from_str(text)?;
        shape::check_no_tags(&raw, "the plan")?;
        let mut plan: Self = serde_yaml::from_value(raw)?;
        plan.check_shape()?;
        plan.resolve_shipped()?;
        Ok(plan)
    }

    /// Remove each bundle whose name is in `existing`, or whose folder is under `bundles_dir`,
    /// and return their names (Q5).
    ///
    /// # Errors
    /// The folder of a bundle cannot be checked, for a reason other than that it is absent.
    pub(crate) fn skip_existing(
        &mut self,
        existing: &BTreeSet<String>,
        bundles_dir: &Path,
    ) -> anyhow::Result<Vec<String>> {
        let mut skipped = Vec::new();
        let mut keep = Vec::new();
        for bundle in std::mem::take(&mut self.bundles) {
            let dir = bundles_dir.join(&bundle.name);
            if existing.contains(&bundle.name) || super::path_exists(&dir)? {
                skipped.push(bundle.name);
            } else {
                keep.push(bundle);
            }
        }
        self.bundles = keep;
        Ok(skipped)
    }

    /// True when the plan adds nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.additions().is_empty()
    }

    /// The additions to `config.yaml`: one `bundle:` entry for each bundle, and the features,
    /// host, and scope entries.
    pub(crate) fn additions(&self) -> Vec<Addition> {
        let mut out = Vec::new();
        let entries: Vec<Value> = self.bundles.iter().map(bundle_entry).collect();
        let lists = [
            ("bundle", None, entries),
            ("features", Some("memory"), self.features.memory.clone()),
            (
                "features",
                Some("codebase_memory"),
                self.features.codebase_memory.clone(),
            ),
        ];
        let scopes = self
            .scope
            .kinds()
            .map(|(kind, entries)| ("scope", Some(kind), entries.clone()));
        for (key, sub, items) in lists.into_iter().chain(scopes) {
            if !items.is_empty() {
                out.push(Addition::Items { key, sub, items });
            }
        }
        if !self.host.is_empty() {
            let keys = self.host.clone();
            out.push(Addition::Keys {
                key: "host",
                sub: None,
                keys,
            });
        }
        out
    }

    /// One summary line per new bundle, with the tags that make it fire.
    pub(crate) fn bundle_lines(&self) -> Vec<String> {
        self.bundles
            .iter()
            .map(|b| {
                let kind = if b.shipped {
                    "shipped bundle"
                } else {
                    "bundle"
                };
                format!("{kind} {} (fires on: {})", b.name, shown_list(&b.when))
            })
            .collect()
    }

    /// One summary line per new scope entry, with the tags it emits.
    pub(crate) fn scope_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (kind, entries) in self.scope.kinds() {
            for entry in entries {
                let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
                let tags: Vec<String> = entry
                    .get("tags")
                    .and_then(Value::as_sequence)
                    .map(|t| {
                        t.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(format!(
                    "{kind} scope {} (emits: {})",
                    shown(id),
                    shown_list(&tags)
                ));
            }
        }
        out
    }

    /// The host names that the plan adds. The shape check makes each key a string.
    pub(crate) fn host_names(&self) -> Vec<String> {
        self.host
            .keys()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect()
    }

    /// The files to create, with paths relative to the config folder: the `bundle.yaml` of a
    /// bundle with capabilities and its `AGENTS.md`, or each file of a shipped bundle.
    ///
    /// # Errors
    /// The capabilities of a bundle cannot be encoded as YAML.
    pub(crate) fn files(&self) -> anyhow::Result<Vec<NewFile>> {
        let mut out = Vec::new();
        for bundle in &self.bundles {
            let dir = Path::new("bundles").join(&bundle.name);
            if bundle.shipped {
                out.extend(shipped_files(bundle, &dir)?);
                continue;
            }
            if !bundle.capabilities.is_empty() {
                out.push(NewFile {
                    rel: dir.join("bundle.yaml"),
                    text: encode_capabilities(bundle)?,
                    executable: false,
                });
            }
            if !bundle.agents_md.is_empty() {
                out.push(NewFile {
                    rel: dir.join("AGENTS.md"),
                    text: bundle.agents_md.clone(),
                    executable: false,
                });
            }
        }
        Ok(out)
    }

    /// The new bundle names.
    pub(crate) fn bundle_names(&self) -> Vec<String> {
        self.bundles.iter().map(|b| b.name.clone()).collect()
    }
}

fn encode_capabilities(bundle: &PlanBundle) -> anyhow::Result<String> {
    serde_yaml::to_string(&without_empty(&bundle.capabilities))
        .with_context(|| format!("encoding bundle.yaml of {}", bundle.name))
}

/// `caps` without the lists and mappings that are empty, such as a `hooks` list whose every entry
/// got a no, so the written file holds only real entries. The second level covers
/// `permissions.allow`.
fn without_empty(caps: &Mapping) -> Mapping {
    let is_empty = |v: &Value| {
        v.as_sequence().is_some_and(Vec::is_empty) || v.as_mapping().is_some_and(Mapping::is_empty)
    };
    let mut out = Mapping::new();
    for (key, value) in caps {
        let value = match value.as_mapping() {
            Some(inner) => Value::Mapping(
                inner
                    .iter()
                    .filter(|(_, v)| !is_empty(v))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            None => value.clone(),
        };
        if !is_empty(&value) {
            out.insert(key.clone(), value);
        }
    }
    out
}

fn bundle_entry(bundle: &PlanBundle) -> Value {
    let mut map = Mapping::new();
    map.insert(Value::from("name"), Value::from(bundle.name.clone()));
    let when: Vec<Value> = bundle.when.iter().cloned().map(Value::from).collect();
    map.insert(Value::from("when"), Value::Sequence(when));
    Value::Mapping(map)
}

/// Text from the plan, safe to print: a control character, such as a terminal escape, could
/// hide the gated text that follows it, so it is shown escaped.
pub(crate) fn shown(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn shown_list(items: &[String]) -> String {
    if items.is_empty() {
        return "no tag".to_string();
    }
    items
        .iter()
        .map(|i| shown(i))
        .collect::<Vec<_>>()
        .join(", ")
}

mod gate;
mod shape;
mod shipped;

use shipped::shipped_files;

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod tests;
