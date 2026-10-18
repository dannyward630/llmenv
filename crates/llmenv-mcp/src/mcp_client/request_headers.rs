//! Build the configured default headers for an MCP HTTP client.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

/// Validate each configured header and collect them into a default header map.
///
/// Header names ignore case, so two keys that differ only by case name the same header. The
/// map would keep only one of them, so that is an error instead.
///
/// # Errors
/// Returns an error when a name or a value is not valid HTTP, or when two keys name the same
/// header.
pub(super) fn build(headers: &BTreeMap<String, String>) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (key, value) in headers {
        let name = HeaderName::from_bytes(key.as_bytes()).with_context(|| {
            format!("MCP header name '{key}' is not valid. Use a plain name such as `x-api-key`.")
        })?;
        if map.contains_key(&name) {
            bail!(
                "MCP header '{key}' is set more than once. Header names ignore case, so keep one \
                 of the keys that differ only by case."
            );
        }
        let mut value = HeaderValue::from_str(value).with_context(|| {
            format!("MCP header '{key}' has a value that is not valid. Use printable ASCII only.")
        })?;
        value.set_sensitive(true);
        map.insert(name, value);
    }
    Ok(map)
}
