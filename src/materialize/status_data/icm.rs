//! The ICM stats collector for the status line: a best-effort live read of the memory counts.

use super::*;

/// This collector runs on hot paths (materialize, `llmenv export`, session
/// start) — not an interactive CLI invocation where a user is watching and
/// waiting is fine. A slow/unreachable ICM backend must not stall one of
/// those paths, so this uses the same short timeout the hook-run MCP calls
/// use rather than the 10s interactive-CLI timeout `memory::stats_json` uses.
const ICM_STATS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Best-effort live ICM query. Returns `None` when no memory backend is
/// active for the current scope, the MCP call fails or times out, or the
/// response can't be parsed — every one of those is an expected, non-error
/// condition (most sessions have no ICM backend configured at all).
pub(super) fn collect_icm() -> Option<IcmData> {
    let raw = match crate::memory::stats_json_with_timeout(ICM_STATS_TIMEOUT) {
        Ok(raw) => raw,
        // Most scopes have no ICM backend, so that case is not a problem to report.
        Err(e) if crate::memory::is_no_backend(&e) => {
            tracing::debug!("icm stat collection skipped, no memory backend for this scope: {e}");
            return None;
        }
        Err(e) => {
            tracing::warn!(
                "icm stat collection failed, so the ICM field is empty: {e:#}. Fix: check that \
                 the ICM backend is running and reachable."
            );
            return None;
        }
    };
    let parsed = parse_icm_stats(&raw);
    if parsed.is_none() {
        // The status field is empty without this line, so a format change in ICM's reply is
        // visible instead of silent. The reply is cut to keep the log short.
        let head: String = raw.chars().take(200).collect();
        tracing::warn!(
            "icm stat reply has no readable Memories line, so the ICM field is empty. Reply \
             starts: {head:?}. Fix: check that the ICM backend returns the stats text."
        );
    }
    parsed
}

/// Pure text-extraction half of `collect_icm`, split out so it's testable
/// without a live ICM MCP backend.
///
/// The `icm_memory_stats` MCP tool has no structured-output mode — it always
/// returns a human-formatted text block (`"Memories: 2880\nTopics: 155\n..."`),
/// never JSON. `memories` is required; `concepts` (sourced from the tool's
/// "Topics" line — its closest analogous count) defaults to 0 when the line
/// is absent, so an older/trimmed stats response still yields a usable count.
pub(super) fn parse_icm_stats(raw: &str) -> Option<IcmData> {
    Some(IcmData {
        memories: extract_stat_line(raw, "Memories")?,
        concepts: extract_stat_line(raw, "Topics").unwrap_or(0),
    })
}

/// Extract the `u64` value from a `"<label>: <n>"` line, e.g. `"Memories:
/// 2880"` for `label = "Memories"`. Whitespace-tolerant around the colon;
/// `None` if no line matches or the value doesn't parse.
pub(super) fn extract_stat_line(raw: &str, label: &str) -> Option<u64> {
    raw.lines().find_map(|line| {
        line.trim()
            .strip_prefix(label)?
            .trim_start()
            .strip_prefix(':')?
            .trim()
            .parse()
            .ok()
    })
}
