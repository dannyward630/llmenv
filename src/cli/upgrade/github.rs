//! GitHub release lookups for `llmenv upgrade`. Every request goes through `get_text`,
//! so a failure names the URL that failed.

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;

use super::GhRelease;

/// The public GitHub API base. Any other base means `LLMENV_UPGRADE_GITHUB_API` is set.
const DEFAULT_API_BASE: &str = "https://api.github.com";

/// Characters of an unexpected response body to show. Enough to recognize a captive
/// portal or a proxy page, short enough to keep the error on one screen.
const BODY_EXCERPT_CHARS: usize = 200;
/// Bytes read from a failed response. A GitHub JSON error is far smaller than this.
const ERROR_BODY_READ_CAP: u64 = 4096;

/// Fetch the latest non-prerelease GitHub release.
pub(super) fn fetch_latest(
    client: &reqwest::blocking::Client,
    base_url: &str,
) -> Result<GhRelease> {
    let url = format!("{base_url}/repos/phaedrus1992/llmenv/releases/latest");
    let not_found = " (no stable release is published, or the repository was not found; \
                      --track beta lists pre-releases)";
    let text = get_text(client, base_url, &url, not_found)?;
    parse_json(&url, &text)
}

/// Fetch releases and return the first non-draft (beta track).
pub(super) fn fetch_beta(client: &reqwest::blocking::Client, base_url: &str) -> Result<GhRelease> {
    let url = format!("{base_url}/repos/phaedrus1992/llmenv/releases?per_page=10");
    let text = get_text(client, base_url, &url, "")?;
    let releases: Vec<GhRelease> = parse_json(&url, &text)?;
    releases
        .into_iter()
        .find(|r| !r.draft)
        .with_context(|| format!("no published releases found at {url} on the beta track"))
}

/// GET a GitHub API URL and return its body. A non-success status names the URL and quotes the
/// start of the error body, which GitHub uses to explain a 403 rate limit. `not_found_hint`
/// is added to a 404. It also names the override when `base_url` is not the public API, so a
/// wrong base path, a proxy, and a real missing release do not look the same.
fn get_text(
    client: &reqwest::blocking::Client,
    base_url: &str,
    url: &str,
    not_found_hint: &str,
) -> Result<String> {
    let resp = client
        .get(url)
        .send()
        .with_context(|| format!("failed to query GitHub releases API at {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let excerpt = read_error_excerpt(resp);
        let hint = failure_hint(status, base_url, not_found_hint);
        bail!("GitHub API returned {status} for {url}{hint}; body starts with {excerpt:?}");
    }
    resp.text()
        .with_context(|| format!("failed to read the GitHub API response from {url}"))
}

/// The start of a failed response body. At most `ERROR_BODY_READ_CAP` bytes are read, so a
/// hostile or broken endpoint cannot stream an unbounded error body into memory.
pub(super) fn read_error_excerpt(resp: reqwest::blocking::Response) -> String {
    let mut bytes = Vec::new();
    let mut capped = std::io::Read::take(resp, ERROR_BODY_READ_CAP);
    match std::io::Read::read_to_end(&mut capped, &mut bytes) {
        Ok(_) => {
            let body = String::from_utf8_lossy(&bytes);
            body.chars().take(BODY_EXCERPT_CHARS).collect()
        }
        Err(e) => format!("(error body unreadable: {e})"),
    }
}

/// The extra advice for a failed GitHub request, chosen by its status and base URL.
fn failure_hint(status: reqwest::StatusCode, base_url: &str, not_found_hint: &str) -> String {
    let mut hint = String::new();
    if status == reqwest::StatusCode::FORBIDDEN {
        hint.push_str(" (rate limited or access denied; wait, then retry)");
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        hint.push_str(not_found_hint);
    }
    if base_url != DEFAULT_API_BASE {
        hint.push_str(" (check LLMENV_UPGRADE_GITHUB_API, which is set)");
    }
    hint
}

/// Parse a GitHub response body. On failure the error shows the start of the body.
fn parse_json<T: DeserializeOwned>(url: &str, text: &str) -> Result<T> {
    serde_json::from_str(text).with_context(|| {
        let excerpt: String = text.chars().take(BODY_EXCERPT_CHARS).collect();
        format!("failed to parse the GitHub response from {url}; body starts with {excerpt:?}")
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        // A parse error quotes a prefix of the body, at most BODY_EXCERPT_CHARS characters long.
        #[test]
        fn parse_error_quotes_a_bounded_prefix_of_the_body(body in "\\PC*") {
            prop_assume!(serde_json::from_str::<serde_json::Value>(&body).is_err());
            let err = parse_json::<serde_json::Value>("https://example.test/x", &body).unwrap_err();
            let excerpt: String = body.chars().take(BODY_EXCERPT_CHARS).collect();
            let msg = format!("{err:#}");
            let quoted = format!("{excerpt:?}");
            prop_assert!(msg.contains(&quoted));
        }
    }

    #[test]
    fn forbidden_hints_at_rate_limit() {
        let hint = failure_hint(reqwest::StatusCode::FORBIDDEN, DEFAULT_API_BASE, "");
        assert!(
            hint.contains("rate limited"),
            "403 must explain the limit: {hint}"
        );
    }

    #[test]
    fn not_found_adds_the_given_hint_only_for_404() {
        let stable = " (try --track beta)";
        let hint = failure_hint(reqwest::StatusCode::NOT_FOUND, DEFAULT_API_BASE, stable);
        assert_eq!(hint, stable);
        let other = failure_hint(reqwest::StatusCode::BAD_GATEWAY, DEFAULT_API_BASE, stable);
        assert!(
            other.is_empty(),
            "a 502 must not suggest a track: {other:?}"
        );
    }

    #[test]
    fn overridden_base_names_the_override_variable() {
        let hint = failure_hint(reqwest::StatusCode::BAD_GATEWAY, "http://proxy.test", "");
        assert!(hint.contains("LLMENV_UPGRADE_GITHUB_API"), "{hint}");
    }
}
