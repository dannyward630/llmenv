use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::onboarding::plan::Plan;

fn err(text: &str) -> String {
    format!("{:#}", Plan::parse(text).unwrap_err())
}

fn shipped_plan(entry: &str) -> Plan {
    Plan::parse(&format!("bundles: [{entry}]")).unwrap_or_else(|e| panic!("{e:#}"))
}

#[test]
fn a_shipped_bundle_takes_its_tags_and_entries_from_the_binary() {
    let p = shipped_plan("{name: rust-dev, shipped: true}");
    assert_eq!(
        p.bundle_lines(),
        ["shipped bundle rust-dev (fires on: lang-rust)"]
    );
    let items = p.gated_items().unwrap();
    let count = |label: &str| items.iter().filter(|(l, _)| l == label).count();
    assert_eq!(count("permission allow in bundle rust-dev"), 4, "{items:?}");
    assert_eq!(count("hook in bundle rust-dev"), 1, "{items:?}");
    assert_eq!(
        count("environment variable in bundle rust-dev"),
        3,
        "{items:?}"
    );
    assert!(
        items
            .iter()
            .any(|(_, text)| text == "RUSTC_WRAPPER: sccache"),
        "{items:?}"
    );
    let overridden = shipped_plan("{name: web-dev, shipped: true, when: [web]}");
    assert_eq!(
        overridden.bundle_lines(),
        ["shipped bundle web-dev (fires on: web)"]
    );
}

#[test]
fn a_shipped_bundle_is_written_as_shipped_until_an_entry_gets_a_no() {
    let embedded = |path: &str| {
        crate::onboarding::shipped::files("rust-dev")
            .find(|f| f.path == path)
            .unwrap()
            .text
    };
    let text_of = |p: &Plan, path: &str| {
        let rel = PathBuf::from("bundles/rust-dev").join(path);
        p.files()
            .unwrap()
            .into_iter()
            .find(|f| f.rel == rel)
            .unwrap()
            .text
    };
    let mut p = shipped_plan("{name: rust-dev, shipped: true}");
    assert_eq!(text_of(&p, "bundle.yaml"), embedded("bundle.yaml"));
    assert_eq!(
        p.files().unwrap().len(),
        crate::onboarding::shipped::files("rust-dev").count()
    );
    let wrapper = p
        .gated_items()
        .unwrap()
        .iter()
        .position(|(_, text)| text == "RUSTC_WRAPPER: sccache")
        .unwrap();
    p.remove_items(&BTreeSet::from([wrapper])).unwrap();
    let written = text_of(&p, "bundle.yaml");
    assert!(
        written.starts_with("# The shipped llmenv bundle rust-dev, without the entries"),
        "{written}"
    );
    let caps: serde_yaml::Value = serde_yaml::from_str(&written).unwrap();
    assert!(caps["env"].get("RUSTC_WRAPPER").is_none(), "{written}");
    assert_eq!(caps["env"]["RUST_BACKTRACE"], serde_yaml::Value::from("1"));
    assert_eq!(text_of(&p, "rules/rust.md"), embedded("rules/rust.md"));
}

#[test]
fn a_shipped_bundle_with_plan_files_or_an_unknown_name_is_refused() {
    let text = err("bundles: [{name: rust-dev, shipped: true, agents_md: x}]");
    assert!(
        text.contains("remove `agents_md` and `capabilities`"),
        "{text}"
    );
    let text = err("bundles: [{name: rust-dev, shipped: true, capabilities: {hooks: []}}]");
    assert!(text.contains("is shipped"), "{text}");
    let text = err("bundles: [{name: cobol-dev, shipped: true}]");
    assert!(
        text.contains("no shipped bundle is named cobol-dev"),
        "{text}"
    );
}

proptest::proptest! {
    /// After any set of no answers to the entries of rust-dev, bundle.yaml is the shipped text
    /// exactly when every answer is yes, and otherwise parses to the remaining entries, with the
    /// remaining env keys in their order.
    #[test]
    fn the_written_bundle_yaml_holds_exactly_the_yes_answers(
        answers in proptest::collection::vec(proptest::prelude::any::<bool>(), 8),
    ) {
        let mut p = shipped_plan("{name: rust-dev, shipped: true}");
        let items = p.gated_items().unwrap();
        proptest::prop_assert_eq!(items.len(), answers.len());
        let env_yes: Vec<String> = items
            .iter()
            .zip(&answers)
            .filter(|((label, _), yes)| label.starts_with("environment") && **yes)
            .map(|((_, text), _)| text.split(':').next().unwrap().to_string())
            .collect();
        let declined: BTreeSet<usize> =
            answers.iter().enumerate().filter(|(_, yes)| !**yes).map(|(i, _)| i).collect();
        p.remove_items(&declined).unwrap();
        let written = p
            .files()
            .unwrap()
            .into_iter()
            .find(|f| f.rel == std::path::Path::new("bundles/rust-dev/bundle.yaml"))
            .unwrap()
            .text;
        let shipped = crate::onboarding::shipped::files("rust-dev")
            .find(|f| f.path == "bundle.yaml")
            .unwrap()
            .text;
        proptest::prop_assert_eq!(written == shipped, declined.is_empty());
        let parsed: serde_yaml::Value = serde_yaml::from_str(&written).unwrap();
        let count = |v: &serde_yaml::Value| v.as_sequence().map_or(0, Vec::len);
        let entries = count(&parsed["permissions"]["allow"]) + count(&parsed["hooks"]);
        let env_keys: Vec<String> = parsed
            .get("env")
            .and_then(serde_yaml::Value::as_mapping)
            .map(|m| m.keys().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        proptest::prop_assert_eq!(&env_keys, &env_yes);
        proptest::prop_assert_eq!(entries + env_keys.len(), answers.len() - declined.len());
    }
}
