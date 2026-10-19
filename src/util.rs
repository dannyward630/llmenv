pub(crate) use llmenv_util::normalize_yaml;
pub use llmenv_util::{
    dedup, escape_control, expand_env_refs, merge_json, merge_yaml, strip_unsafe_chars,
};

/// A control character, or an invisible format character that can reorder or hide text.
fn needs_escape(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206F}'
                | '\u{FEFF}'
        )
}

/// `s` in single quotes, with each embedded single quote closed, escaped, and reopened. The
/// shell expands nothing inside, so the text reaches the command as it is.
pub(crate) fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `path` as one shell word, so a command in a message can be pasted as it is.
pub(crate) fn shell_word(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@%".contains(c));
    if plain {
        text
    } else if text.is_empty() {
        // An empty word would vanish from the pasted command, so it is written as a quoted empty.
        "''".to_string()
    } else {
        single_quote(&text)
    }
}

/// Escape every C0/C1 control character in `s` (`char::is_control`) so it's
/// safe to print verbatim to a terminal (#1076). Config-derived strings
/// (`native_permissions.*` keys, permission tool names and patterns,
/// bundle/marketplace names) can originate from a shared or marketplace
/// `bundle.yaml` since #1072 widened validation to the merged manifest — a
/// key or pattern containing an ANSI escape or a carriage return could
/// otherwise rewrite or hide surrounding `doctor`/`export` output on
/// someone else's terminal. No exceptions for `\n`/`\t`: a legitimate
/// config key, tool name, or pattern has no reason to contain either.
///
/// Invisible Unicode format characters are escaped too (#2669 review). They are not control
/// characters, but a bidi override (U+202E) can reorder the text around a key, and a
/// zero-width character can hide part of it.
///
/// Returns `s` unchanged (no allocation) when nothing needs escaping.
pub(crate) fn display_safe(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.chars().any(needs_escape) {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if needs_escape(c) {
            out.push_str(&format!("\\u{{{:04x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Lexically normalize a path: resolve `..` and `.` components without touching
/// the filesystem. This prevents `..`-based traversal bypasses when checking
/// path prefixes on paths that may not exist yet.
pub(crate) fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Pure path-prefix check: does `expanded_path` lie within `cache_root`? Components are compared,
/// not text, so `/cache2` is not within `/cache`.
///
/// Normalizes `..` components lexically (no filesystem access) so traversal
/// paths like `/cache/llmenv/../../../etc/passwd` cannot bypass the check.
#[must_use]
pub(crate) fn is_within(cache_root: &std::path::Path, expanded_path: &str) -> bool {
    let path = normalize_path(std::path::Path::new(expanded_path));
    path.starts_with(cache_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_safe_returns_borrowed_for_plain_text() {
        assert!(matches!(
            display_safe("plain text"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn display_safe_escapes_ansi_escape() {
        let out = display_safe("Bash(\x1b[2Kevil*)");
        assert!(!out.contains('\x1b'));
        assert!(out.contains("\\u{001b}"));
    }

    #[test]
    fn display_safe_escapes_bidi_override_and_zero_width() {
        let out = display_safe("key\u{202E}evil\u{200B}");
        assert!(!out.contains('\u{202E}'));
        assert!(!out.contains('\u{200B}'));
        assert!(out.contains("\\u{202e}"));
        assert!(out.contains("\\u{200b}"));
    }

    #[test]
    fn display_safe_keeps_ordinary_non_ascii_text() {
        assert!(matches!(
            display_safe("naïve 日本語"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn display_safe_escapes_carriage_return() {
        let out = display_safe("native_permissions\rhidden");
        assert!(!out.contains('\r'));
        assert!(out.contains("\\u{000d}"));
    }

    #[test]
    fn display_safe_escapes_newline_and_tab_too() {
        // No exception for \n/\t: a config key/tool name/pattern has no
        // legitimate reason to contain either.
        let out = display_safe("a\nb\tc");
        assert!(!out.contains('\n') && !out.contains('\t'));
    }

    proptest::proptest! {
        #[test]
        fn display_safe_never_panics(s in ".{0,100}") {
            let _ = display_safe(&s);
        }

        #[test]
        fn display_safe_output_never_contains_a_raw_control_char(s in ".{0,100}") {
            let out = display_safe(&s);
            proptest::prop_assert!(!out.chars().any(char::is_control));
        }
    }
}
