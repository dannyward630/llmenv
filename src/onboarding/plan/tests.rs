use std::collections::BTreeSet;
use std::path::PathBuf;

use super::Plan;
use crate::onboarding::config_splice::Addition;

const PLAN: &str = r#"
bundles:
  - name: rust-dev
    when: [lang-rust]
    files:
      AGENTS.md: "Use cargo nextest."
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
          handler: {type: command, command: hooks/fmt.sh}
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
        hook.contains("command: hooks/fmt.sh") && hook.contains("PostToolUse"),
        "{hook}"
    );
}

#[test]
fn a_declined_item_is_removed_from_the_files_and_the_additions() {
    let mut p = plan();
    p.remove_items(&BTreeSet::from([0, 3]));
    let items = p.gated_items().unwrap();
    assert_eq!(items.len(), 2, "{items:?}");
    assert!(
        items[0].1.contains("rg *"),
        "the second allow stays: {items:?}"
    );
    let files = p.files().unwrap();
    let bundle_yaml = &files
        .iter()
        .find(|(f, _)| f.ends_with("bundle.yaml"))
        .unwrap()
        .1;
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
fn files_live_under_the_bundle_folder() {
    let files = plan().files().unwrap();
    let paths: Vec<PathBuf> = files.iter().map(|(p, _)| p.clone()).collect();
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
    p.skip_existing(&BTreeSet::from(["rust-dev".to_string()]), dir.path());
    assert_eq!(p.skipped(), ["rust-dev"]);
    assert!(p.bundle_names().is_empty());

    let mut p = plan();
    std::fs::create_dir(dir.path().join("rust-dev")).unwrap();
    p.skip_existing(&BTreeSet::new(), dir.path());
    assert_eq!(p.skipped(), ["rust-dev"]);
    assert!(!p.is_empty(), "the feature entry is still in the plan");
}

#[test]
fn shape_errors_name_the_fix() {
    assert!(err("bundles: [{name: ../x}]").contains("not a plain name"));
    assert!(err("bundles: [{name: a}, {name: a}]").contains("twice"));
    assert!(err("bundles: [{name: a, files: {../x: y}}]").contains("relative path inside"));
    assert!(err("bundles: [{name: a, files: {/etc/x: y}}]").contains("relative path inside"));
    assert!(err("bundles: [{name: a, files: {bundle.yaml: y}}]").contains("set `capabilities`"));
    let native = err("bundles: [{name: a, capabilities: {native_hooks: {}}}]");
    assert!(
        native.contains("cannot set `native_hooks`") && native.contains("Fix:"),
        "{native}"
    );
    assert!(err("bundle: []").contains("unknown field"));
    assert!(err("features: {throttle: {}}").contains("unknown field"));
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
    assert_eq!(p.scope_ids(), ["content rust"]);
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
