//! The `display` presets of the statusline widgets, checked when the config loads (#3059).

use super::ValidateError;

/// The `display` presets that each widget accepts (see `WidgetConfig::display`). A widget not
/// listed here has no display mode, and it ignores the field.
const WIDGET_DISPLAY_PRESETS: &[(&str, &[&str])] = &[
    ("usage_5h", &["clock_24h", "clock_12h"]),
    ("usage_7d", &["clock_24h", "clock_12h"]),
    ("model", &["short", "version", "full"]),
    ("pr", &["number", "url"]),
];

/// Reject a `display` value that the widget does not accept. Without this check, a typo falls
/// back to the default with no error (#3059).
///
/// # Errors
/// Returns [`ValidateError::WidgetDisplayInvalid`] when `value` is not a preset of `widget`.
pub fn check_widget_display(widget: &str, value: &str) -> Result<(), ValidateError> {
    let Some((_, allowed)) = WIDGET_DISPLAY_PRESETS
        .iter()
        .find(|(name, _)| *name == widget)
    else {
        return Ok(());
    };
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(ValidateError::WidgetDisplayInvalid {
        widget: widget.to_owned(),
        value: value.to_owned(),
        allowed: allowed.join(", "),
    })
}
