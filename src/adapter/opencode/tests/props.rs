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

/// A naive recursive glob over `*` and `?`, the oracle for the matcher. It does not call the
/// code under test.
fn naive_glob(pattern: &[char], input: &[char]) -> bool {
    match pattern.split_first() {
        None => input.is_empty(),
        Some(('*', rest)) => {
            naive_glob(rest, input)
                || input
                    .split_first()
                    .is_some_and(|(_, tail)| naive_glob(pattern, tail))
        }
        Some(('?', rest)) => input
            .split_first()
            .is_some_and(|(_, tail)| naive_glob(rest, tail)),
        Some((c, rest)) => input
            .split_first()
            .is_some_and(|(x, tail)| x == c && naive_glob(rest, tail)),
    }
}

/// Every string over `{a, b}` of length 0 to `max`.
fn words_up_to(max: usize) -> Vec<Vec<char>> {
    let mut all = vec![Vec::new()];
    let mut last = vec![Vec::new()];
    for _ in 0..max {
        last = last
            .iter()
            .flat_map(|w: &Vec<char>| {
                ['a', 'b'].map(|c| {
                    let mut next = w.clone();
                    next.push(c);
                    next
                })
            })
            .collect();
        all.extend(last.iter().cloned());
    }
    all
}

fn rule(tool: &str, pattern: &str) -> crate::config::PermissionRule {
    crate::config::PermissionRule {
        tool: tool.into(),
        pattern: Some(pattern.into()),
        paths: vec![],
    }
}

fn arb_rules() -> impl Strategy<Value = Vec<crate::config::PermissionRule>> {
    proptest::collection::vec(
        (
            prop::sample::select(vec!["Bash", "Read", "Edit"]),
            prop::sample::select(vec!["*", "git *", "git push *", "ls", "a?c"]),
        ),
        0..4,
    )
    .prop_map(|v| v.into_iter().map(|(t, p)| rule(t, p)).collect())
}

proptest! {
    /// The matcher agrees with the naive oracle on every short pattern and input.
    #[test]
    fn the_glob_matcher_agrees_with_the_oracle(
        pattern in "[ab*?]{0,6}",
        input in "[ab]{0,6}",
    ) {
        let p: Vec<char> = pattern.chars().collect();
        let i: Vec<char> = input.chars().collect();
        prop_assert_eq!(permissions::glob_matches(&pattern, &i), naive_glob(&p, &i));
    }

    /// A subsumption verdict is sound: every input the narrow pattern matches, the broad one
    /// matches too. A false verdict would hide a real ordering inversion.
    #[test]
    fn a_subsumption_verdict_is_sound(broad in "[ab*?]{0,4}", narrow in "[ab*?]{0,4}") {
        if permissions::pattern_subsumes(&broad, &narrow) {
            let b: Vec<char> = broad.chars().collect();
            let n: Vec<char> = narrow.chars().collect();
            for word in words_up_to(5) {
                prop_assert!(
                    !naive_glob(&n, &word) || naive_glob(&b, &word),
                    "{:?} matches {:?} but {:?} does not", narrow, word, broad
                );
            }
        }
    }

    /// Two patterns that share an input are seen to overlap.
    #[test]
    fn patterns_that_share_an_input_overlap(a in "[ab*?]{0,4}", b in "[ab*?]{0,4}") {
        let pa: Vec<char> = a.chars().collect();
        let pb: Vec<char> = b.chars().collect();
        if words_up_to(4).iter().any(|w| naive_glob(&pa, w) && naive_glob(&pb, w)) {
            prop_assert!(permissions::patterns_overlap(&a, &b), "{:?} and {:?}", a, b);
        }
    }

    /// The order of the rules inside one tier does not change the rendered list.
    #[test]
    fn the_rule_order_inside_a_tier_does_not_matter(
        allow in arb_rules(),
        deny in arb_rules(),
        seed in any::<u64>(),
    ) {
        let perms = |allow: Vec<_>, deny: Vec<_>| crate::config::Permissions {
            allow,
            deny,
            ..crate::config::Permissions::default()
        };
        let mut shuffled = allow.clone();
        let len = shuffled.len().max(1);
        shuffled.rotate_left(usize::try_from(seed % len as u64).unwrap_or(0));
        let mut deny_rev = deny.clone();
        deny_rev.reverse();
        let one = permissions::render(&perms(allow, deny), None);
        let two = permissions::render(&perms(shuffled, deny_rev), None);
        match (one, two) {
            (Ok(a), Ok(b)) => prop_assert_eq!(a, b),
            (Err(_), Err(_)) => {}
            (a, b) => prop_assert!(false, "{:?} vs {:?}", a.is_ok(), b.is_ok()),
        }
    }
}
