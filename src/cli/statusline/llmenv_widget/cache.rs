//! The `cache` widget: the prunable cache size, humanized or raw.

use super::humanize::humanize_bytes;
use crate::cli::statusline::data::StatusData;

pub(super) fn render_cache(data: &StatusData, cfg: Option<&llmenv_config::WidgetConfig>) -> String {
    let Some(cache) = &data.cache else {
        return String::new();
    };
    // `None` means the size could not be read. The marker keeps the field visible.
    let humanized = cache
        .prunable_bytes
        .map_or_else(|| UNKNOWN_MARKER.to_string(), humanize_bytes);
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
