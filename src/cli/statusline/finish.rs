//! Statusline finish helper: applies per-widget style and wraps output.

/// Apply the per-widget style and wrap the raw text with style metadata.
///
/// `name` is the widget name used for style lookup. `raw` is the rendered
/// text. `cfg` is optional widget config. `dyn_style` is an optional
/// runtime-derived style (for the `pr` widget). `use_color` gates color.
pub(super) fn finish(
    name: &str,
    raw: String,
    cfg: Option<&llmenv_config::StatuslineConfig>,
    dyn_style: Option<&str>,
    use_color: bool,
) -> String {
    let style = dyn_style
        .or_else(|| Some(default_style(name)))
        .unwrap_or("white");
    crate::cli::style::apply_style(&raw, style, use_color)
}

/// Return the default style for a widget name.
fn default_style(name: &str) -> &'static str {
    match name {
        "model" | "folder" | "branch" | "duration" => "cyan",
        "tokens" => "yellow",
        "budget" => "green",
        "cache_usage" => "blue",
        "context" => "magenta",
        "usage_5h" | "usage_7d" => "green",
        "peak" => "white",
        "pr" => "yellow",
        _ => "white",
    }
}
