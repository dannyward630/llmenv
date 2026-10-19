//! Generated-input checks of the opencode native rule parser and the agent translation.

use proptest::prelude::*;

use super::*;

fn native_allow(rule: &str) -> crate::config::NativePermissionRules {
    crate::config::NativePermissionRules {
        allow: vec![rule.to_string()],
        ask: vec![],
        deny: vec![],
    }
}

fn render_native(rule: &str) -> anyhow::Result<Option<Vec<permissions::Rule>>> {
    permissions::render(
        &crate::config::Permissions::default(),
        Some(&native_allow(rule)),
    )
}

proptest! {
    /// `tool(pattern)` renders one rule with that action and that resource. The tool names
    /// start with `x`, so no 1.x name is renamed, and the pattern may hold parentheses.
    #[test]
    fn a_native_rule_renders_its_tool_and_pattern(
        tool in "x[a-z_]{0,8}",
        pattern in "[a-z *./()-]{0,10}[a-z*]",
    ) {
        let rules = render_native(&format!("{tool}({pattern})")).unwrap().unwrap();
        prop_assert_eq!(rules.len(), 1);
        prop_assert_eq!(&rules[0].action, &tool);
        prop_assert_eq!(&rules[0].resource, &pattern);
        prop_assert_eq!(rules[0].effect.as_str(), "allow");
    }

    /// Any rule string renders or fails with an error, and never panics. A rule that renders
    /// names a tool with no whitespace and a resource that is not empty.
    #[test]
    fn any_native_rule_renders_or_errors(rule in "\\PC{0,24}") {
        if let Ok(Some(rules)) = render_native(&rule) {
            for r in &rules {
                prop_assert!(!r.action.is_empty() && !r.action.contains(char::is_whitespace));
                prop_assert!(!r.resource.is_empty());
            }
        }
    }

    /// Text after the closing `)` is an error, not a dropped tail.
    #[test]
    fn text_after_the_closing_paren_is_an_error(tail in "[a-z]{1,5}") {
        let rule = format!("xtool(ls) {tail}");
        prop_assert!(render_native(&rule).is_err());
    }

    /// The agent translation keeps `description` and `model`, adds `mode: subagent`, and keeps
    /// the body, for any plain values.
    #[test]
    fn an_agent_keeps_its_description_model_and_body(
        description in "[A-Za-z][A-Za-z ]{0,20}[A-Za-z]",
        model in "[a-z][a-z0-9-]{0,10}",
        body in "[A-Za-z][A-Za-z .\n]{0,40}",
    ) {
        let source = format!("---\ndescription: {description}\nmodel: {model}\n---\n{body}");
        let out = translate::translate_agent_md(&source).unwrap();
        let (fm, out_body) = translate::split_frontmatter(&out);
        let fm = fm.unwrap();
        let map = fm.as_mapping().unwrap();
        let get = |k: &str| map.get(serde_yaml::Value::String(k.into())).and_then(|v| v.as_str());
        prop_assert_eq!(get("description"), Some(description.as_str()));
        prop_assert_eq!(get("model"), Some(model.as_str()));
        prop_assert_eq!(get("mode"), Some("subagent"));
        prop_assert_eq!(out_body, body.as_str());
    }

    /// The frontmatter split and both translations never panic on any text.
    #[test]
    fn the_translations_never_panic(source in "\\PC{0,60}") {
        let _ = translate::split_frontmatter(&source);
        let _ = translate::translate_agent_md(&source);
        let _ = translate::translate_command_md(&source);
    }
}
