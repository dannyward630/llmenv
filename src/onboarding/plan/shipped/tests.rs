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
