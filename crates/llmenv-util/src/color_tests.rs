use super::*;

#[test]
fn test_should_use_color_always_mode() {
    assert!(should_use_color(Some(ColorMode::Always), false));
    assert!(should_use_color(Some(ColorMode::Always), true));
}

#[test]
fn test_should_use_color_never_mode() {
    assert!(!should_use_color(Some(ColorMode::Never), false));
    assert!(!should_use_color(Some(ColorMode::Never), true));
}

#[test]
fn test_should_use_color_auto_respects_tty() {
    assert!(!should_use_color(Some(ColorMode::Auto), false));
}

#[test]
fn test_should_use_color_auto_with_tty_isolated() {
    let no_env = |_name: &str| -> Option<String> { None };
    assert!(!should_use_color_with_env(
        Some(ColorMode::Auto),
        false,
        &no_env
    ));
    assert!(should_use_color_with_env(
        Some(ColorMode::Auto),
        true,
        &no_env
    ));
}

#[test]
fn test_should_use_color_no_color_overrides() {
    let no_color_env = |name: &str| -> Option<String> {
        match name {
            "NO_COLOR" => Some("1".to_string()),
            _ => None,
        }
    };
    assert!(!should_use_color_with_env(
        Some(ColorMode::Auto),
        true,
        &no_color_env
    ));
}

#[test]
fn test_should_use_color_no_color_empty_string() {
    let no_color_empty_env = |name: &str| -> Option<String> {
        match name {
            "NO_COLOR" => Some(String::new()),
            _ => None,
        }
    };
    assert!(!should_use_color_with_env(
        Some(ColorMode::Auto),
        true,
        &no_color_empty_env
    ));
}

#[test]
fn test_should_use_color_clicolor_force_overrides() {
    let force_env = |name: &str| -> Option<String> {
        match name {
            "CLICOLOR_FORCE" => Some("1".to_string()),
            _ => None,
        }
    };
    assert!(should_use_color_with_env(
        Some(ColorMode::Auto),
        false,
        &force_env
    ));
}

#[test]
fn test_should_use_color_clicolor_force_empty_string_does_not_force() {
    let empty_force_env = |name: &str| -> Option<String> {
        match name {
            "CLICOLOR_FORCE" => Some(String::new()),
            _ => None,
        }
    };
    assert!(!should_use_color_with_env(
        Some(ColorMode::Auto),
        false,
        &empty_force_env
    ));
}

#[test]
fn test_should_use_color_no_color_takes_precedence_over_clicolor_force() {
    let both_env = |name: &str| -> Option<String> {
        match name {
            "NO_COLOR" => Some("1".to_string()),
            "CLICOLOR_FORCE" => Some("1".to_string()),
            _ => None,
        }
    };
    assert!(!should_use_color_with_env(
        Some(ColorMode::Auto),
        true,
        &both_env
    ));
}

#[test]
fn doctor_warning_plain_when_no_color() {
    assert_eq!(doctor_warning(false), "⚠");
}

#[test]
fn doctor_warning_colored_contains_escape_code() {
    assert!(doctor_warning(true).contains('\u{1b}'));
}
