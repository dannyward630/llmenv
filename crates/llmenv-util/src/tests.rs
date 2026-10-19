use super::{dedup, escape_control, merge_json, merge_yaml, normalize_json, strip_unsafe_chars};

#[test]
fn strip_unsafe_chars_removes_control_bidi_and_zero_width_characters() {
    let spoof = "safe\u{202E}txt.exe\u{200B}\u{FEFF}\u{1b}[31m\n\tend\u{2066}";
    assert_eq!(strip_unsafe_chars(spoof), "safetxt.exe[31mend");
}

#[test]
fn strip_unsafe_chars_keeps_ordinary_unicode() {
    let text = "café 日本語 📄 — naïve";
    assert_eq!(strip_unsafe_chars(text), text);
}

mod strip_props {
    use super::strip_unsafe_chars;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn output_has_no_control_or_bidi_characters(text in ".*") {
            let out = strip_unsafe_chars(&text);
            let has_control = out.chars().any(char::is_control);
            let has_bidi_or_zero_width = out.contains('\u{202E}') || out.contains('\u{200B}');
            prop_assert!(!has_control);
            prop_assert!(!has_bidi_or_zero_width);
        }

        #[test]
        fn stripping_twice_changes_nothing(text in ".*") {
            let once = strip_unsafe_chars(&text);
            prop_assert_eq!(strip_unsafe_chars(&once), once);
        }
    }
}

// A newline must not forge a second log line, and an ESC byte must not reach the terminal.
#[test]
fn escape_control_neutralizes_newline_and_escape_bytes() {
    let forged = "path=a\n[LLMENV_CACHE] content_hash hit 0.001ms";
    let escaped = escape_control(forged);
    assert!(!escaped.contains('\n'), "newline must not survive escaping");
    assert_eq!(escaped, r"path=a\n[LLMENV_CACHE] content_hash hit 0.001ms");
    assert!(!escape_control("a\u{1b}[2Jb").contains('\u{1b}'));
}

#[test]
fn escape_control_leaves_non_control_unicode_untouched() {
    let path = "path=café_日本語_📄.md";
    assert_eq!(escape_control(path), path);
}

#[test]
fn escape_control_escapes_a_bidi_override_and_a_zero_width_space() {
    assert_eq!(
        escape_control("a\u{202E}b\u{200B}c"),
        "a\\u{202e}b\\u{200b}c"
    );
}

#[test]
fn expand_env_refs_expands_plain_and_braced_names() {
    let env = |name: &str| (name == "A_1").then(|| "x".to_string());
    assert_eq!(
        super::expand_env_refs("$A_1/b", &env).as_deref(),
        Some("x/b")
    );
    assert_eq!(
        super::expand_env_refs("${A_1}b", &env).as_deref(),
        Some("xb")
    );
    assert_eq!(
        super::expand_env_refs("no vars", &env).as_deref(),
        Some("no vars")
    );
}

#[test]
fn expand_env_refs_rejects_a_bare_dollar_an_unclosed_brace_and_an_unset_name() {
    let env = |_: &str| Some("x".to_string());
    for bad in ["$", "a$/b", "${A", "${A b}", "${}", "$-"] {
        assert_eq!(super::expand_env_refs(bad, &env), None, "{bad}");
    }
    assert_eq!(super::expand_env_refs("$A", &|_| None), None);
    assert_eq!(super::expand_env_refs("$A", &|_| Some(String::new())), None);
}

mod escape_control_props {
    use super::escape_control;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn output_has_no_control_chars(s in any::<String>()) {
            prop_assert!(!escape_control(&s).chars().any(char::is_control));
        }

        #[test]
        fn control_free_text_is_unchanged(s in "\\PC{0,40}") {
            prop_assume!(!s.chars().any(char::is_control));
            prop_assume!(!s.chars().any(super::super::is_unsafe_format_char));
            prop_assert_eq!(escape_control(&s), s);
        }

        #[test]
        fn output_has_no_bidi_or_zero_width_chars(s in any::<String>()) {
            prop_assert!(
                !escape_control(&s).chars().any(super::super::is_unsafe_format_char)
            );
        }
    }
}

fn yaml(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

#[test]
fn merge_yaml_unions_disjoint_mapping_keys() {
    let mut dst = yaml("a: 1");
    merge_yaml(&mut dst, yaml("b: 2"));
    assert_eq!(dst, yaml("a: 1\nb: 2"));
}

#[test]
fn merge_yaml_concatenates_and_dedups_sequences() {
    let mut dst = yaml("- one\n- two");
    merge_yaml(&mut dst, yaml("- two\n- three"));
    assert_eq!(dst, yaml("- one\n- two\n- three"));
}

#[test]
fn merge_yaml_recurses_into_shared_mapping_keys() {
    let mut dst = yaml("outer:\n  a: 1\n  list: [x]");
    merge_yaml(&mut dst, yaml("outer:\n  b: 2\n  list: [y]"));
    assert_eq!(dst, yaml("outer:\n  a: 1\n  b: 2\n  list: [x, y]"));
}

#[test]
fn merge_yaml_src_scalar_overwrites_dst() {
    let mut dst = yaml("k: old");
    merge_yaml(&mut dst, yaml("k: new"));
    assert_eq!(dst, yaml("k: new"));
}

#[test]
fn merge_yaml_shape_mismatch_src_wins() {
    let mut dst = yaml("k: [a, b]");
    merge_yaml(&mut dst, yaml("k: scalar"));
    assert_eq!(dst, yaml("k: scalar"));
}

fn jsn(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn merge_json_unions_disjoint_object_keys() {
    let mut dst = jsn(r#"{"a": 1}"#);
    merge_json(&mut dst, jsn(r#"{"b": 2}"#));
    assert_eq!(dst, jsn(r#"{"a": 1, "b": 2}"#));
}

#[test]
fn merge_json_concatenates_and_dedups_arrays() {
    let mut dst = jsn(r#"["one", "two"]"#);
    merge_json(&mut dst, jsn(r#"["two", "three"]"#));
    assert_eq!(dst, jsn(r#"["one", "two", "three"]"#));
}

#[test]
fn merge_json_recurses_into_shared_object_keys() {
    let mut dst = jsn(r#"{"outer": {"a": 1, "list": ["x"]}}"#);
    merge_json(&mut dst, jsn(r#"{"outer": {"b": 2, "list": ["y"]}}"#));
    assert_eq!(
        dst,
        jsn(r#"{"outer": {"a": 1, "b": 2, "list": ["x", "y"]}}"#)
    );
}

#[test]
fn merge_json_src_scalar_overwrites_dst() {
    let mut dst = jsn(r#"{"k": "old"}"#);
    merge_json(&mut dst, jsn(r#"{"k": "new"}"#));
    assert_eq!(dst, jsn(r#"{"k": "new"}"#));
}

#[test]
fn merge_json_shape_mismatch_src_wins() {
    let mut dst = jsn(r#"{"k": ["a", "b"]}"#);
    merge_json(&mut dst, jsn(r#"{"k": "scalar"}"#));
    assert_eq!(dst, jsn(r#"{"k": "scalar"}"#));
}

#[test]
fn normalize_json_strips_null_keys() {
    let mut v = jsn(r#"{"a": null, "b": 1, "c": {"d": null, "e": [{"f": null, "g": 2}]}}"#);
    normalize_json(&mut v);
    assert_eq!(v, jsn(r#"{"b": 1, "c": {"e": [{"g": 2}]}}"#));
}

#[test]
fn merge_json_dedups_when_null_vs_absent_differs() {
    // Objects differing only by null vs absent key (e.g. "tool": null
    // from an older render vs absent key from the current) must collapse
    // during merge_json's PartialEq-based dedup. This is the #699/#718
    // fix: normalize_json strips nulls first so the compare succeeds.
    let mut dst = jsn(r#"[{"command": "test", "tool": null}]"#);
    let src = jsn(r#"[{"command": "test"}]"#);
    merge_json(&mut dst, src);
    assert_eq!(dst, jsn(r#"[{"command": "test"}]"#));
}

#[test]
fn removes_later_duplicates_preserving_order() {
    let mut v = vec!["a", "b", "a", "c", "b"];
    dedup(&mut v);
    assert_eq!(v, vec!["a", "b", "c"]);
}

#[test]
fn empty_and_singleton_are_noops() {
    let mut empty: Vec<i32> = Vec::new();
    dedup(&mut empty);
    assert!(empty.is_empty());
    let mut one = vec![1];
    dedup(&mut one);
    assert_eq!(one, vec![1]);
}

#[test]
fn idempotent() {
    let mut v = vec![1, 1, 2, 3, 3, 3];
    dedup(&mut v);
    let once = v.clone();
    dedup(&mut v);
    assert_eq!(v, once);
}

mod props;
mod yaml_props;
