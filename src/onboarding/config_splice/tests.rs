use proptest::prelude::*;
use serde_yaml::{Mapping, Value};

use super::{Addition, splice};

fn yaml(text: &str) -> Value {
    serde_yaml::from_str(text).unwrap_or_else(|e| panic!("bad test yaml {text:?}: {e}"))
}

fn bundle(name: &str, tag: &str) -> Addition {
    Addition::Items {
        key: "bundle",
        sub: None,
        items: vec![yaml(&format!("{{name: {name}, when: [{tag}]}}"))],
    }
}

fn run(old: &str, additions: &[Addition]) -> (String, String) {
    splice(old, additions).unwrap_or_else(|e| panic!("splice failed: {e:#}"))
}

/// Every line of `old` is still in `new`, in the same order.
fn keeps_old_lines(old: &str, new: &str) -> bool {
    let mut new_lines = new.lines();
    old.lines().all(|l| new_lines.any(|n| n == l))
}

#[test]
fn an_indentless_bundle_list_gets_the_new_item_and_keeps_comments() {
    let old = "# my config\nbundle:\n- name: base\n  when:\n  - me\n# trailing note\nmcp: []\n";
    let (new, diff) = run(old, &[bundle("rust-dev", "lang-rust")]);
    assert!(keeps_old_lines(old, &new), "{new}");
    assert!(new.contains("- name: rust-dev"), "{new}");
    let names: Vec<String> = yaml(&new)["bundle"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["base", "rust-dev"]);
    assert!(
        diff.contains("+ - name: rust-dev") && !diff.contains("\n- "),
        "{diff}"
    );
    let note = new.find("# trailing note").unwrap();
    assert!(
        new.find("rust-dev").unwrap() < note,
        "the item goes above the trailing comment"
    );
}

#[test]
fn an_indented_bundle_list_keeps_its_indent() {
    let old = "bundle:\n  - name: base\n    when: [me]\n";
    let (new, _) = run(old, &[bundle("x", "t")]);
    assert!(new.contains("\n  - name: x"), "{new}");
    assert_eq!(yaml(&new)["bundle"].as_sequence().unwrap().len(), 2);
}

#[test]
fn an_absent_key_is_added_at_the_end() {
    let old = "mcp: []\n";
    let (new, _) = run(old, &[bundle("x", "t")]);
    assert!(new.starts_with("mcp: []\n"), "{new}");
    assert_eq!(yaml(&new)["bundle"][0]["name"], Value::from("x"));
}

#[test]
fn an_empty_value_gets_its_block_and_the_diff_shows_the_removed_line() {
    for empty in ["null", "~", "[]"] {
        let old = format!("bundle: {empty}\nmcp: []\n");
        let (new, diff) = run(&old, &[bundle("x", "t")]);
        assert_eq!(yaml(&new)["bundle"][0]["name"], Value::from("x"), "{new}");
        assert!(diff.contains(&format!("- bundle: {empty}")), "{diff}");
    }
}

fn memory_entry() -> Addition {
    Addition::Items {
        key: "features",
        sub: Some("memory"),
        items: vec![yaml("{server_host: still, port: 9092, when: [me]}")],
    }
}

#[test]
fn a_sub_key_is_added_to_an_existing_block_or_created() {
    let with_list =
        "features:\n  memory:\n  - server_host: a\n    port: 1\n  codebase_memory: []\n";
    let (new, _) = run(with_list, &[memory_entry()]);
    assert_eq!(
        yaml(&new)["features"]["memory"]
            .as_sequence()
            .unwrap()
            .len(),
        2
    );
    assert!(keeps_old_lines(with_list, &new), "{new}");

    let without = "features:\n  codebase_memory: []\nhost: {}\n";
    let (new, _) = run(without, &[memory_entry()]);
    assert_eq!(
        yaml(&new)["features"]["memory"][0]["port"],
        Value::from(9092)
    );

    let empty_sub = "features:\n  memory: []\n";
    let (new, _) = run(empty_sub, &[memory_entry()]);
    assert_eq!(
        yaml(&new)["features"]["memory"]
            .as_sequence()
            .unwrap()
            .len(),
        1
    );

    let null_parent = "features: null\n";
    let (new, _) = run(null_parent, &[memory_entry()]);
    assert_eq!(
        yaml(&new)["features"]["memory"]
            .as_sequence()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn host_keys_are_added_to_a_mapping_block() {
    let old = "host:\n  laptop:\n    addr: 10.0.0.1\n";
    let mut keys = Mapping::new();
    keys.insert(Value::from("still"), yaml("{addr: still.local}"));
    let add = Addition::Keys {
        key: "host",
        sub: None,
        keys,
    };
    let (new, _) = run(old, &[add]);
    let host = &yaml(&new)["host"];
    assert_eq!(host["laptop"]["addr"], Value::from("10.0.0.1"));
    assert_eq!(host["still"]["addr"], Value::from("still.local"));
}

#[test]
fn a_flow_list_is_refused_with_the_lines_to_add_by_hand() {
    let old = "bundle: [{name: base, when: [me]}]\n";
    let err = splice(old, &[bundle("x", "t")]).unwrap_err();
    let text = format!("{err:#}");
    assert!(
        text.contains("add these lines to config.yaml by hand"),
        "{text}"
    );
    assert!(text.contains("name: x"), "{text}");
}

#[test]
fn several_additions_apply_in_order() {
    let old = "bundle:\n- name: base\n  when: [me]\nfeatures: null\n";
    let (new, diff) = run(old, &[bundle("a", "t"), bundle("b", "t"), memory_entry()]);
    let v = yaml(&new);
    assert_eq!(v["bundle"].as_sequence().unwrap().len(), 3);
    assert_eq!(v["features"]["memory"].as_sequence().unwrap().len(), 1);
    assert_eq!(diff.matches("@@").count(), 6, "{diff}");
}

proptest! {
    /// Any block list of bundles takes new items, keeps every old line, and parses to the old
    /// list plus the new items.
    #[test]
    fn a_block_list_always_takes_new_bundles(
        old_names in prop::collection::btree_set("[a-z]{1,8}", 0..5),
        new_names in prop::collection::btree_set("[a-z]{1,8}", 1..4),
        indent in 0usize..3,
        comment in any::<bool>(),
    ) {
        let pad = " ".repeat(indent * 2);
        let mut old = String::from("bundle:\n");
        for name in &old_names {
            old.push_str(&format!("{pad}- name: {name}\n{pad}  when: [me]\n"));
        }
        if comment {
            old.push_str("# end of bundles\n");
        }
        old.push_str("mcp: []\n");
        let additions: Vec<Addition> = new_names.iter().map(|n| bundle(n, "t")).collect();
        let (new, _) = splice(&old, &additions).unwrap();
        prop_assert!(keeps_old_lines(&old, &new));
        let count = yaml(&new)["bundle"].as_sequence().map_or(0, Vec::len);
        prop_assert_eq!(count, old_names.len() + new_names.len());
    }
}
