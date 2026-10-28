//! The `cache` widget: the prunable cache size, humanized or raw.

use super::humanize::humanize_bytes;
use crate::cli::statusline::data::StatusData;

pub(super) fn render_cache(data: &StatusData, cfg: Option<&llmenv_config::WidgetConfig>) -> String {
    let Some(cache) = &data.cache else {
        return String::new();
    };
    // `None` means the size could not be read. The marker keeps the field visible.
    // A partial total is a floor, so "at least" keeps it from reading as exact (#3069).
    let humanized = cache.prunable_bytes.map_or_else(
        || UNKNOWN_MARKER.to_string(),
        |bytes| {
            if cache.prunable_partial {
                format!("at least {}", humanize_bytes(bytes))
            } else {
                humanize_bytes(bytes)
            }
        },
    );
    let raw = cache
        .prunable_bytes
        .map_or_else(|| UNKNOWN_MARKER.to_string(), |bytes| bytes.to_string());
    let format = cfg
        .and_then(|c| c.format.as_deref())
        .unwrap_or("{prunable}");
    format
        .replace("{prunable}", &humanized)
        .replace("{prunable_raw}", &raw)
}

/// Shown in place of a size that could not be read.
pub(super) const UNKNOWN_MARKER: &str = "?";
