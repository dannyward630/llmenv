use std::collections::BTreeSet;
use std::path::PathBuf;

use proptest::prelude::*;

use super::Plan;
use crate::onboarding::config_splice::Addition;

const PLAN: &str = r#"
bundles:
  - name: rust-dev
    when: [lang-rust]
    agents_md: "Use cargo nextest."
    capabilities:
      permissions:
        allow:
          - {tool: Bash, pattern: "cargo *"}
          - {tool: Bash, pattern: "rg *"}
        deny:
          - {tool: Bash, pattern: "rm *"}
      hooks:
        - event: PostToolUse
          matcher: Edit
          handler: {type: command, command: cargo fmt}
features:
  codebase_memory:
    - {when: [me]}
"#;

fn plan() -> Plan {
    Plan::parse(PLAN).unwrap_or_else(|e| panic!("{e:#}"))
}

fn err(text: &str) -> String {
    format!("{:#}", Plan::parse(text).unwrap_err())
}

#[test]
fn the_gated_items_are_the_allows_hooks_and_features_in_order() {
    let items = plan().gated_items().unwrap();
    let labels: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(
        labels,
        [
            "permission allow in bundle rust-dev",
            "permission allow in bundle rust-dev",
            "hook in bundle rust-dev",
            "codebase-memory-mcp server (features.codebase_memory)",
        ]
    );
    let (_, hook) = &items[2];
    assert!(
        hook.contains("command: cargo fmt") && hook.contains("PostToolUse"),
        "{hook}"
    );
}

#[test]
fn a_declined_item_is_removed_from_the_files_and_the_additions() {
    let mut p = plan();
    p.remove_items(&BTreeSet::from([0, 3])).unwrap();
    let items = p.gated_items().unwrap();
    assert_eq!(items.len(), 2, "{items:?}");
    assert!(
        items[0].1.contains("rg *"),
        "the second allow stays: {items:?}"
    );
    let files = p.files().unwrap();
    let bundle_yaml = &files
        .iter()
        .find(|f| f.rel.ends_with("bundle.yaml"))
        .unwrap()
        .text;
    assert!(
        !bundle_yaml.contains("cargo *") && bundle_yaml.contains("rg *"),
        "{bundle_yaml}"
    );
    assert!(
        bundle_yaml.contains("rm *"),
        "a deny is not gated and stays: {bundle_yaml}"
    );
    let features = p.additions().into_iter().filter(|a| {
        matches!(
            a,
            Addition::Items {
                key: "features",
                ..
            }
        )
    });
    assert_eq!(features.count(), 0, "the declined feature entry is gone");
}

#[test]
fn a_declined_position_outside_the_plan_is_an_error() {
    let err = plan().remove_items(&BTreeSet::from([99])).unwrap_err();
    assert!(format!("{err:#}").contains("not in the plan"), "{err:#}");
}

#[test]
fn a_host_is_a_gated_item_and_a_no_removes_it() {
    let mut p = Plan::parse("host: {still: {addr: still.local}}").unwrap();
    let items = p.gated_items().unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        items[0].0 == "host still" && items[0].1.contains("still.local"),
        "{items:?}"
    );
    p.remove_items(&BTreeSet::from([0])).unwrap();
    assert!(p.is_empty());
}

#[test]
fn files_live_under_the_bundle_folder() {
    let files = plan().files().unwrap();
    let paths: Vec<PathBuf> = files.iter().map(|f| f.rel.clone()).collect();
    assert_eq!(
        paths,
        [
            PathBuf::from("bundles/rust-dev/bundle.yaml"),
            PathBuf::from("bundles/rust-dev/AGENTS.md")
        ]
    );
}

#[test]
fn the_additions_list_each_bundle_and_feature() {
    let additions = plan().additions();
    assert_eq!(additions.len(), 2);
    let Addition::Items {
        key: "bundle",
        items,
        ..
    } = &additions[0]
    else {
        panic!("{additions:?}");
    };
    assert_eq!(items[0]["name"], serde_yaml::Value::from("rust-dev"));
    assert_eq!(items[0]["when"][0], serde_yaml::Value::from("lang-rust"));
}

#[test]
fn an_existing_bundle_is_skipped_by_name_or_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = plan();
    let skipped = p
        .skip_existing(&BTreeSet::from(["rust-dev".to_string()]), dir.path())
        .unwrap();
    assert_eq!(skipped, ["rust-dev"]);
    assert!(p.bundle_names().is_empty());

    let mut p = plan();
    std::fs::create_dir(dir.path().join("rust-dev")).unwrap();
    assert_eq!(
        p.skip_existing(&BTreeSet::new(), dir.path()).unwrap(),
        ["rust-dev"]
    );
    assert!(!p.is_empty(), "the feature entry is still in the plan");
}

#[test]
fn shape_errors_name_the_fix() {
    assert!(err("bundles: [{name: ../x}]").contains("not a plain name"));
    assert!(err("bundles: [{name: a}, {name: a}]").contains("twice"));
    assert!(err("bundles: [{name: a, files: {x: y}}]").contains("unknown field"));
    let native = err("bundles: [{name: a, capabilities: {native_hooks: {}}}]");
    assert!(
        native.contains("cannot set `native_hooks`") && native.contains("Fix:"),
        "{native}"
    );
    assert!(err("bundle: []").contains("unknown field"));
    assert!(err("features: {throttle: {}}").contains("unknown field"));
    assert!(err("scope: {content: [{match: {glob: x}}]}").contains("has no `id`"));
    assert!(err("host: {1: {addr: x}}").contains("not a string"));
}

/// Each of these slipped past the gate before the shape rules (security review of #2962).
#[test]
fn entries_that_could_skip_the_gate_are_refused() {
    let cases = [
        (
            "bundles: [{name: a, capabilities: {!x permissions: {allow: [{tool: Bash}]}}}]",
            "YAML tag",
        ),
        (
            "bundles: [{name: a, capabilities: {hooks: !h [{event: Stop}]}}]",
            "YAML tag",
        ),
        (
            "bundles: [{name: a, capabilities: {permissions: {default_mode: bypassPermissions}}}]",
            "permissions.default_mode",
        ),
        (
            "bundles: [{name: a, capabilities: {permissions: {preset: safe-readonly}}}]",
            "permissions.preset",
        ),
        (
            "bundles: [{name: a, capabilities: {env: {BASH_ENV: /tmp/x}}}]",
            "cannot set `env`",
        ),
        (
            "bundles: [{name: a, capabilities: {skills: [x]}}]",
            "cannot set `skills`",
        ),
        (
            "bundles: [{name: a, capabilities: {hooks: {event: Stop}}}]",
            "must be a list",
        ),
        (
            "bundles: [{name: a, capabilities: {permissions: {allow: {tool: Bash}}}}]",
            "must be a list",
        ),
        (
            "bundles: [{name: a, files: {Bundle.yaml: x}}]",
            "unknown field",
        ),
    ];
    for (plan, expected) in cases {
        let text = err(plan);
        assert!(text.contains(expected), "{plan}: {text}");
    }
}

#[test]
fn plan_text_is_shown_with_control_characters_escaped() {
    let p =
        Plan::parse("scope: {user: [{id: \"a\\e[2Jb\", match: {user: me}, tags: [t]}]}").unwrap();
    let lines = p.scope_lines();
    assert!(
        !lines[0].contains('\u{1b}') && lines[0].contains("\\u{1b}"),
        "{lines:?}"
    );
    assert!(lines[0].contains("emits: t"), "{lines:?}");
    assert_eq!(
        plan().bundle_lines(),
        ["bundle rust-dev (fires on: lang-rust)"]
    );
}

#[test]
fn an_empty_plan_is_empty() {
    assert!(Plan::parse("{}").unwrap().is_empty());
    assert!(
        Plan::parse("host: {h: {addr: x}}")
            .map(|p| !p.is_empty())
            .unwrap()
    );
}

#[test]
fn scope_entries_are_added_without_a_question() {
    let p =
        Plan::parse("scope: {content: [{id: rust, match: {glob: Cargo.toml}, tags: [lang-rust]}]}")
            .unwrap();
    assert!(!p.is_empty());
    assert!(p.gated_items().unwrap().is_empty());
    assert_eq!(p.scope_lines(), ["content scope rust (emits: lang-rust)"]);
    let additions = p.additions();
    assert!(
        matches!(
            &additions[..],
            [Addition::Items {
                key: "scope",
                sub: Some("content"),
                ..
            }]
        ),
        "{additions:?}"
    );
}

proptest! {
    /// After any set of no answers, the gated entries left are exactly the ones that got a yes,
    /// in order. The model is a filter over the first list.
    #[test]
    fn remove_items_leaves_exactly_the_yes_answers(answers in prop::collection::vec(any::<bool>(), 4)) {
        let mut p = plan();
        let before = p.gated_items().unwrap();
        let declined: BTreeSet<usize> =
            answers.iter().enumerate().filter(|(_, yes)| !**yes).map(|(i, _)| i).collect();
        p.remove_items(&declined).unwrap();
        let expected: Vec<(String, String)> = before
            .into_iter()
            .zip(&answers)
            .filter(|(_, yes)| **yes)
            .map(|(item, _)| item)
            .collect();
        prop_assert_eq!(p.gated_items().unwrap(), expected);
    }
}

/// The limit is 1 MiB: a plan with long rules text loads, and a larger file is refused.
#[test]
fn a_plan_up_to_the_size_limit_loads_and_a_larger_one_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan.yaml");
    let plan_of = |rules: usize| {
        format!(
            "bundles: [{{name: a, agents_md: \"{}\"}}]\n",
            "x".repeat(rules)
        )
    };
    std::fs::write(&path, plan_of(512 * 1024)).unwrap();
    assert_eq!(Plan::load(&path).unwrap().bundle_names(), ["a"]);
    std::fs::write(&path, plan_of(1024 * 1024)).unwrap();
    let err = format!("{:#}", Plan::load(&path).unwrap_err());
    assert!(err.contains("reading the plan"), "{err}");
}

/// A no to one host keeps the other hosts in their plan order.
#[test]
fn a_declined_host_keeps_the_order_of_the_rest() {
    let mut p = Plan::parse("host: {a: {addr: x}, b: {addr: y}, c: {addr: z}}").unwrap();
    p.remove_items(&BTreeSet::from([0])).unwrap();
    assert_eq!(p.host_names(), ["b", "c"]);
}
