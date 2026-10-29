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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code")]
mod tests {
    use super::super::Config;
    use super::*;
    use crate::{StatuslineConfig, WidgetConfig};

    #[test]
    fn config_validate_rejects_a_bad_widget_display() {
        let widget = WidgetConfig {
            display: Some("clock_24H".to_string()),
            ..Default::default()
        };
        let cfg = Config {
            statusline: Some(StatuslineConfig {
                widgets: std::collections::BTreeMap::from([("usage_5h".to_string(), widget)]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let err = cfg
            .validate()
            .expect_err("a bad display must fail validation");

        assert!(
            matches!(&err, ValidateError::WidgetDisplayInvalid { widget, .. } if widget == "usage_5h"),
            "the error must name the widget"
        );
    }
}
