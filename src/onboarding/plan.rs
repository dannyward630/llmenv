//! The onboarding plan that `llmenv setup --apply` reads (#2788). The skill writes it; this
//! module parses it, checks its shape, lists the entries that need a yes, and gives the files
//! and the `config.yaml` additions to write.
//! Design: docs/design/issue-2773-onboarding.md ("The plan file and `llmenv setup --apply`").

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, bail, ensure};
use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use super::config_splice::Addition;

/// A plan holds rules text and a few entries. 1 MiB is far above any real plan, and it stops a
/// runaway file before it fills memory.
const MAX_PLAN_BYTES: u64 = 1024 * 1024;

/// The capability keys that a plan bundle may set. The gate cannot show what the other keys do,
/// so they are added by hand after onboarding.
const PLAN_CAPABILITY_KEYS: [&str; 8] = [
    "permissions",
    "hooks",
    "plugins",
    "mcp",
    "lsp",
    "skills",
    "output_styles",
    "env",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanBundle {
    name: String,
    #[serde(default)]
    when: Vec<String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    capabilities: Mapping,
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
    #[serde(skip)]
    skipped: Vec<String>,
}

impl Plan {
    /// Read and check the plan at `path`.
    ///
    /// # Errors
    /// The file cannot be read, is not a valid plan, or breaks a shape rule: a bad bundle name,
    /// a repeated name, an unsafe file path, or a capability key that a plan may not set.
    pub(crate) fn load(path: &Path) -> anyhow::Result<Self> {
        let text = llmenv_paths::read_text_bounded(path, MAX_PLAN_BYTES)
            .with_context(|| format!("reading the plan {}", path.display()))?
            .with_context(|| format!("the plan {} does not exist", path.display()))?;
        Self::parse(&text).with_context(|| format!("the plan {} is not valid", path.display()))
    }

    fn parse(text: &str) -> anyhow::Result<Self> {
        let plan: Self = serde_yaml::from_str(text)?;
        plan.check_shape()?;
        Ok(plan)
    }

    fn check_shape(&self) -> anyhow::Result<()> {
        let mut seen = BTreeSet::new();
        for bundle in &self.bundles {
            let name = &bundle.name;
            ensure!(
                llmenv_paths::is_valid_short_name(name),
                "bundle name {name:?} is not a plain name (letters, digits, `.`, `_`, `-`)"
            );
            ensure!(
                seen.insert(name.clone()),
                "bundle {name:?} is in the plan twice"
            );
            for file in bundle.files.keys() {
                check_file_path(name, file)?;
            }
            for key in bundle.capabilities.keys() {
                let key = key.as_str().unwrap_or_default();
                ensure!(
                    PLAN_CAPABILITY_KEYS.contains(&key),
                    "bundle {name:?}: an onboarding plan cannot set `{key}`. Fix: leave it out, and \
                     add it to bundle.yaml by hand after setup. Allowed keys: {}",
                    PLAN_CAPABILITY_KEYS.join(", ")
                );
            }
        }
        Ok(())
    }

    /// Move each bundle whose name is in `existing`, or whose folder is under `bundles_dir`, to
    /// the skipped list (Q5).
    pub(crate) fn skip_existing(&mut self, existing: &BTreeSet<String>, bundles_dir: &Path) {
        let (skip, keep): (Vec<PlanBundle>, Vec<PlanBundle>) = std::mem::take(&mut self.bundles)
            .into_iter()
            .partition(|b| {
                existing.contains(&b.name) || bundles_dir.join(&b.name).symlink_metadata().is_ok()
            });
        self.skipped.extend(skip.into_iter().map(|b| b.name));
        self.bundles = keep;
    }

    /// The bundles that the plan skipped because they exist.
    pub(crate) fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// True when the plan adds nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.bundles.is_empty()
            && self.features.memory.is_empty()
            && self.features.codebase_memory.is_empty()
            && self.host.is_empty()
            && self
                .scope
                .kinds()
                .iter()
                .all(|(_, entries)| entries.is_empty())
    }

    /// The additions to `config.yaml`: one `bundle:` entry for each bundle, and the features
    /// and host entries.
    pub(crate) fn additions(&self) -> Vec<Addition> {
        let mut out = Vec::new();
        let entries: Vec<Value> = self.bundles.iter().map(bundle_entry).collect();
        if !entries.is_empty() {
            out.push(Addition::Items {
                key: "bundle",
                sub: None,
                items: entries,
            });
        }
        if !self.features.memory.is_empty() {
            let items = self.features.memory.clone();
            out.push(Addition::Items {
                key: "features",
                sub: Some("memory"),
                items,
            });
        }
        if !self.features.codebase_memory.is_empty() {
            let items = self.features.codebase_memory.clone();
            out.push(Addition::Items {
                key: "features",
                sub: Some("codebase_memory"),
                items,
            });
        }
        if !self.host.is_empty() {
            out.push(Addition::Keys {
                key: "host",
                sub: None,
                keys: self.host.clone(),
            });
        }
        for (kind, entries) in self.scope.kinds() {
            if !entries.is_empty() {
                out.push(Addition::Items {
                    key: "scope",
                    sub: Some(kind),
                    items: entries.clone(),
                });
            }
        }
        out
    }

    /// The scope entries that the plan adds, as `kind id` lines.
    pub(crate) fn scope_ids(&self) -> Vec<String> {
        self.scope
            .kinds()
            .iter()
            .flat_map(|(kind, entries)| {
                entries.iter().map(move |e| {
                    format!(
                        "{kind} {}",
                        e.get("id").and_then(Value::as_str).unwrap_or("(no id)")
                    )
                })
            })
            .collect()
    }

    /// The host names that the plan adds.
    pub(crate) fn host_names(&self) -> Vec<String> {
        self.host
            .keys()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect()
    }

    /// The files to create, as paths relative to the config folder, with their text. Each
    /// bundle with capabilities gets a `bundle.yaml`.
    ///
    /// # Errors
    /// The capabilities of a bundle cannot be encoded as YAML.
    pub(crate) fn files(&self) -> anyhow::Result<Vec<(PathBuf, String)>> {
        let mut out = Vec::new();
        for bundle in &self.bundles {
            let dir = Path::new("bundles").join(&bundle.name);
            if !bundle.capabilities.is_empty() {
                let text = serde_yaml::to_string(&bundle.capabilities)
                    .with_context(|| format!("encoding bundle.yaml of {}", bundle.name))?;
                out.push((dir.join("bundle.yaml"), text));
            }
            for (file, text) in &bundle.files {
                out.push((dir.join(file), text.clone()));
            }
        }
        Ok(out)
    }

    /// The new bundle names.
    pub(crate) fn bundle_names(&self) -> Vec<String> {
        self.bundles.iter().map(|b| b.name.clone()).collect()
    }
}

fn bundle_entry(bundle: &PlanBundle) -> Value {
    let mut map = Mapping::new();
    map.insert(Value::from("name"), Value::from(bundle.name.clone()));
    let when: Vec<Value> = bundle.when.iter().cloned().map(Value::from).collect();
    map.insert(Value::from("when"), Value::Sequence(when));
    Value::Mapping(map)
}

/// A bundle file path must be relative, stay in the bundle, and not be `bundle.yaml`, which
/// comes from `capabilities`.
fn check_file_path(bundle: &str, file: &str) -> anyhow::Result<()> {
    let path = Path::new(file);
    let plain = path.components().all(|c| matches!(c, Component::Normal(_)));
    if file.is_empty() || !plain {
        bail!("bundle {bundle:?}: file {file:?} must be a relative path inside the bundle");
    }
    ensure!(
        path != Path::new("bundle.yaml"),
        "bundle {bundle:?}: set `capabilities` for bundle.yaml, not a file named bundle.yaml"
    );
    Ok(())
}

mod gate;

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod tests;
