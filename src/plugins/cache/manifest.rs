//! Reads the manifest of a marketplace checkout: its plugin entries and its declared name.

use std::path::Path;

use anyhow::{Context, Result};

use super::{git_subdir_source, github_repo_source, pin_source, unsupported_kind};

/// A plugin entry parsed from a marketplace's `.claude-plugin/marketplace.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketplacePluginEntry {
    pub(crate) name: String,
    pub(crate) source: String,
    /// For a `git-subdir` source: the directory of the clone that holds the plugin (#2441).
    pub(crate) subdir: Option<String>,
}

/// Parse plugin entries from a marketplace clone's `.claude-plugin/marketplace.json`.
/// Returns an empty vec when the file is absent (bundles without a manifest are valid).
///
/// # Errors
/// Returns an error when the file exists but cannot be read or parsed.
pub(crate) fn read_marketplace_plugins(
    marketplace_dir: &Path,
) -> Result<Vec<MarketplacePluginEntry>> {
    let manifest_path = marketplace_dir
        .join(".claude-plugin")
        .join("marketplace.json");
    // #893: a single read that distinguishes NotFound (→ empty) from other I/O
    // errors (→ propagate), rather than an exists() stat that masked every stat
    // failure (e.g. EACCES) as "no manifest". The marketplace is a cloned repository, so the
    // read is capped and refuses a FIFO (#2945).
    let Some(content) = crate::paths::bounded::asset_text_optional(&manifest_path)? else {
        return Ok(Vec::new());
    };
    let json: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
    let plugins = json
        .get("plugins")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| match parse_plugin_entry(entry) {
                    Ok(parsed) => parsed,
                    // One bad entry must not hide the other plugins of the marketplace.
                    Err(e) => {
                        eprintln!("warning: {e:#} — skipping entry");
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(plugins)
}

/// Parse one `plugins[]` entry of a marketplace manifest. `Ok(None)` skips an entry that cannot
/// be used, with a warning on stderr. An `Err` is a malformed source (github, git-subdir, or a
/// pin), and the caller skips the entry with the error as the warning.
fn parse_plugin_entry(entry: &serde_json::Value) -> Result<Option<MarketplacePluginEntry>> {
    let name = match entry.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            eprintln!(
                "warning: marketplace entry skipped: missing or non-string 'name' \
                 field (entry = {:?})",
                entry
            );
            return Ok(None);
        }
    };
    let raw = match entry.get("source") {
        Some(r) => r,
        None => {
            eprintln!(
                "warning: marketplace entry '{}': missing 'source' field — \
                 skipping entry",
                name
            );
            return Ok(None);
        }
    };
    let mut subdir = None;
    let source = if let Some(s) = raw.as_str() {
        s.to_string()
    } else if raw.get("source").and_then(|v| v.as_str()) == Some("npm") {
        // #1014: an npm-source object ({"source": "npm", "package": ...,
        // "version": ...}) has no URL to clone — Claude Code's own
        // `/plugin install` resolves it directly from the npm registry.
        // Encode it as a source string `is_external_plugin_source`
        // recognizes as non-external (nothing for llmenv to clone),
        // matching the "./" -prefix sentinel this field already uses
        // for local-path sources.
        match raw.get("package").and_then(|v| v.as_str()) {
            Some(pkg) => match raw.get("version").and_then(|v| v.as_str()) {
                Some(version) => format!("npm:{pkg}@{version}"),
                None => format!("npm:{pkg}"),
            },
            None => {
                eprintln!(
                    "warning: marketplace entry '{}': npm-source object has no \
                     string 'package' field (source = {:?}) — skipping entry",
                    name, raw
                );
                return Ok(None);
            }
        }
    } else if let Some(kind) = unsupported_kind(raw) {
        eprintln!(
            "warning: marketplace entry '{name}': source kind '{kind}' is not supported — \
             skipping entry"
        );
        return Ok(None);
    } else if raw.get("source").and_then(|v| v.as_str()) == Some("git-subdir") {
        let (source, path) = git_subdir_source(&name, raw)?;
        subdir = Some(path);
        source
    } else if let Some(url) = raw.get("url").and_then(|v| v.as_str()) {
        pin_source(&name, url, raw)?
    } else if let Some(github) = github_repo_source(&name, raw)? {
        github
    } else {
        eprintln!(
            "warning: marketplace entry '{name}': object-form source has no string 'url' field \
             and no github 'repo' field (source = {raw:?}) — skipping entry"
        );
        return Ok(None);
    };
    Ok(Some(MarketplacePluginEntry {
        name,
        source,
        subdir,
    }))
}

/// The name that a marketplace checkout declares in its manifest. Claude Code names a directory
/// marketplace from this field, not from the config key, so a mismatch hides its plugins (#3048).
/// `None` when the manifest is absent or has no `name`.
///
/// # Errors
/// Returns an error when the file exists but cannot be read or parsed.
pub(crate) fn read_marketplace_name(marketplace_dir: &Path) -> Result<Option<String>> {
    let manifest_path = marketplace_dir
        .join(".claude-plugin")
        .join("marketplace.json");
    let Some(content) = crate::paths::bounded::asset_text_optional(&manifest_path)? else {
        return Ok(None);
    };
    let json: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
    Ok(json
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code")]
mod tests {
    use super::read_marketplace_name;

    #[test]
    fn read_marketplace_name_returns_the_declared_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".claude-plugin")).expect("create manifest dir");
        std::fs::write(
            dir.path().join(".claude-plugin").join("marketplace.json"),
            r#"{"name": "declared"}"#,
        )
        .expect("write manifest");

        let name = read_marketplace_name(dir.path()).expect("read");

        assert_eq!(name.as_deref(), Some("declared"));
    }

    #[test]
    fn read_marketplace_name_is_none_without_a_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(read_marketplace_name(dir.path()).expect("read"), None);
    }
}
