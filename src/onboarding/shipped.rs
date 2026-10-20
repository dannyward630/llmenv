//! The bundles that ship in the llmenv binary (#2787): the manifest `bundles/shipped.yaml` and
//! the embedded files of each bundle folder. An onboarding plan names one with `shipped: true`.
//! Design: docs/design/issue-2773-onboarding.md ("Shipped bundles").

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

mod files;

use files::{Embedded, FILES};

const MANIFEST: &str = include_str!("../../bundles/shipped.yaml");

/// One entry of the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShippedBundle {
    name: String,
    /// The default tags that turn the bundle on. Any one of them is enough.
    pub(crate) when: Vec<String>,
    /// One line for the plan summary.
    purpose: String,
}

/// The manifest, in its file order.
///
/// # Errors
/// The embedded manifest is not valid. A test parses it, so this is a build defect.
pub(crate) fn manifest() -> anyhow::Result<Vec<ShippedBundle>> {
    serde_yaml::from_str(MANIFEST).context("the embedded bundles/shipped.yaml is not valid")
}

/// The manifest entry of the shipped bundle `name`.
///
/// # Errors
/// No shipped bundle has that name. The message lists the names.
pub(crate) fn find(name: &str) -> anyhow::Result<ShippedBundle> {
    let all = manifest()?;
    let names: Vec<String> = all.iter().map(|b| b.name.clone()).collect();
    all.into_iter().find(|b| b.name == name).with_context(|| {
        format!(
            "no shipped bundle is named {}. Fix: use one of {}, or leave out `shipped: true` to \
             write the bundle in the plan",
            super::plan::shown(name),
            names.join(", ")
        )
    })
}

/// The embedded files of the shipped bundle `name`, in path order.
pub(crate) fn files(name: &str) -> impl Iterator<Item = &'static Embedded> {
    FILES.iter().filter(move |f| f.bundle == name)
}

/// The capabilities in the `bundle.yaml` of the shipped bundle `name`. A bundle with no
/// `bundle.yaml`, or one with comments only, has none.
///
/// # Errors
/// The embedded `bundle.yaml` is not a YAML mapping. A test loads each one, so this is a build
/// defect.
pub(crate) fn capabilities(name: &str) -> anyhow::Result<Mapping> {
    let Some(file) = files(name).find(|f| f.path == "bundle.yaml") else {
        return Ok(Mapping::new());
    };
    let value: Value = serde_yaml::from_str(file.text)
        .with_context(|| format!("the embedded bundle.yaml of {name} is not valid"))?;
    match value {
        Value::Null => Ok(Mapping::new()),
        Value::Mapping(map) => Ok(map),
        _ => anyhow::bail!("the embedded bundle.yaml of {name} is not a mapping"),
    }
}

/// One shipped bundle as `llmenv setup --bundles` prints it.
#[derive(Debug, Serialize)]
struct Listing {
    #[serde(flatten)]
    bundle: ShippedBundle,
    files: Vec<&'static str>,
}

/// The shipped bundles as JSON, for the onboarding skill: each manifest entry and its files.
///
/// # Errors
/// The manifest is not valid, or the JSON cannot be encoded.
pub(crate) fn listing_json() -> anyhow::Result<String> {
    let listing: Vec<Listing> = manifest()?
        .into_iter()
        .map(|bundle| {
            let files = files(&bundle.name).map(|f| f.path).collect();
            Listing { bundle, files }
        })
        .collect();
    serde_json::to_string_pretty(&listing).context("encoding the shipped bundle list")
}

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod tests;
