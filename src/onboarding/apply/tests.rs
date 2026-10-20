use std::collections::VecDeque;
use std::path::Path;

use super::{Prompter, apply};

/// Answers from a list, and records each question.
struct Script {
    answers: VecDeque<bool>,
    asked: Vec<String>,
    on_last: Option<Box<dyn FnMut()>>,
}

impl Script {
    fn new(answers: &[bool]) -> Self {
        Self {
            answers: answers.iter().copied().collect(),
            asked: Vec::new(),
            on_last: None,
        }
    }
}

impl Prompter for Script {
    fn confirm(&mut self, question: &str) -> anyhow::Result<bool> {
        self.asked.push(question.to_string());
        let answer = self.answers.pop_front().unwrap_or(false);
        if self.answers.is_empty()
            && let Some(hook) = self.on_last.as_mut()
        {
            hook();
        }
        Ok(answer)
    }
}

const CONFIG: &str = "# my settings\nbundle:\n- name: base\n  when: [me]\n# keep me\nmcp: []\n";

const PLAN: &str = r#"
bundles:
  - name: rust-dev
    when: [lang-rust]
    files: {AGENTS.md: "Use cargo nextest."}
    capabilities:
      permissions:
        allow:
          - {tool: Bash, pattern: "cargo *"}
          - {tool: Bash, pattern: "rg *"}
"#;

struct Setup {
    dir: tempfile::TempDir,
}

impl Setup {
    fn new(config: &str, plan: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.yaml"), config).unwrap();
        std::fs::create_dir_all(dir.path().join("bundles/base")).unwrap();
        std::fs::write(dir.path().join("plan.yaml"), plan).unwrap();
        Self { dir }
    }

    fn path(&self, rel: &str) -> std::path::PathBuf {
        self.dir.path().join(rel)
    }

    fn run(&self, script: &mut Script) -> (anyhow::Result<bool>, String) {
        let mut out = Vec::new();
        let result = apply(&self.path("plan.yaml"), self.dir.path(), script, &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    fn config(&self) -> String {
        std::fs::read_to_string(self.path("config.yaml")).unwrap()
    }
}

#[test]
fn yes_to_everything_creates_the_bundle_and_lists_it() {
    let s = Setup::new(CONFIG, PLAN);
    let mut script = Script::new(&[true, true, true, true]);
    let (result, out) = s.run(&mut script);
    assert!(result.unwrap(), "{out}");
    assert_eq!(
        std::fs::read_to_string(s.path("bundles/rust-dev/AGENTS.md")).unwrap(),
        "Use cargo nextest."
    );
    let bundle = std::fs::read_to_string(s.path("bundles/rust-dev/bundle.yaml")).unwrap();
    assert!(
        bundle.contains("cargo *") && bundle.contains("rg *"),
        "{bundle}"
    );
    let config = s.config();
    assert!(
        config.starts_with("# my settings\nbundle:\n- name: base"),
        "{config}"
    );
    assert!(
        config.contains("# keep me") && config.contains("- name: rust-dev"),
        "{config}"
    );
    assert_eq!(
        script.asked.len(),
        4,
        "plan, two allows, config: {:?}",
        script.asked
    );
    assert!(script.asked[1].starts_with("Allow this permission allow in bundle rust-dev"));
    assert!(
        out.contains("+ - name: rust-dev"),
        "the diff is shown: {out}"
    );
}

#[test]
fn a_no_to_the_plan_writes_nothing() {
    let s = Setup::new(CONFIG, PLAN);
    let (result, out) = s.run(&mut Script::new(&[false]));
    assert!(!result.unwrap());
    assert!(out.contains("Nothing was written"), "{out}");
    assert!(!s.path("bundles/rust-dev").exists());
    assert_eq!(s.config(), CONFIG);
}

#[test]
fn a_no_to_one_item_leaves_only_that_item_out() {
    let s = Setup::new(CONFIG, PLAN);
    let (result, out) = s.run(&mut Script::new(&[true, false, true, true]));
    assert!(result.unwrap(), "{out}");
    let bundle = std::fs::read_to_string(s.path("bundles/rust-dev/bundle.yaml")).unwrap();
    assert!(
        !bundle.contains("cargo *") && bundle.contains("rg *"),
        "{bundle}"
    );
    assert!(out.contains("1 entry is left out"), "{out}");
}

#[test]
fn a_no_to_the_config_change_writes_nothing() {
    let s = Setup::new(CONFIG, PLAN);
    let (result, out) = s.run(&mut Script::new(&[true, true, true, false]));
    assert!(!result.unwrap());
    assert!(out.contains("never fires"), "{out}");
    assert!(!s.path("bundles/rust-dev").exists());
    assert_eq!(s.config(), CONFIG);
}

#[test]
fn an_existing_bundle_is_skipped_and_reported() {
    let plan = "bundles: [{name: base, when: [me], files: {AGENTS.md: x}}]";
    let s = Setup::new(CONFIG, plan);
    let mut script = Script::new(&[]);
    let (result, out) = s.run(&mut script);
    assert!(!result.unwrap());
    assert!(
        out.contains("Skipped bundle base") && out.contains("adds nothing"),
        "{out}"
    );
    assert!(
        script.asked.is_empty(),
        "nothing to ask: {:?}",
        script.asked
    );
    assert!(!s.path("bundles/base/AGENTS.md").exists());
}

#[test]
fn a_config_edited_during_the_run_is_not_overwritten() {
    let s = Setup::new(CONFIG, PLAN);
    let config_path = s.path("config.yaml");
    let mut script = Script::new(&[true, true, true, true]);
    script.on_last = Some(Box::new(move || {
        std::fs::write(&config_path, format!("{CONFIG}# edited\n")).unwrap();
    }));
    let (result, _) = s.run(&mut script);
    let err = format!("{:#}", result.unwrap_err());
    assert!(err.contains("changed while the plan ran"), "{err}");
    assert!(s.config().ends_with("# edited\n"));
}

#[test]
fn a_scalar_collision_with_an_existing_bundle_fails_before_any_question() {
    let s = Setup::new(
        CONFIG,
        "bundles: [{name: two, when: [x], capabilities: {permissions: {default_mode: plan}}}]",
    );
    std::fs::write(
        s.path("bundles/base/bundle.yaml"),
        "permissions:\n  default_mode: acceptEdits\n",
    )
    .unwrap();
    let mut script = Script::new(&[true]);
    let (result, _) = s.run(&mut script);
    let err = format!("{:#}", result.unwrap_err());
    assert!(err.contains("do not merge"), "{err}");
    assert!(script.asked.is_empty());
}

#[test]
fn a_host_that_exists_is_refused() {
    let config = "bundle:\n- name: base\n  when: [me]\nhost:\n  still:\n    addr: 10.0.0.1\n";
    let s = Setup::new(config, "host: {still: {addr: still.local}}");
    let err = format!("{:#}", s.run(&mut Script::new(&[])).0.unwrap_err());
    assert!(err.contains("already has") && err.contains("Fix:"), "{err}");
}

#[test]
fn a_missing_config_names_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    let plan = dir.path().join("plan.yaml");
    std::fs::write(&plan, PLAN).unwrap();
    let mut out = Vec::new();
    let err = apply(&plan, dir.path(), &mut Script::new(&[]), &mut out).unwrap_err();
    assert!(
        format!("{err:#}").contains("run `llmenv setup` first"),
        "{err:#}"
    );
    assert!(!Path::new(&dir.path().join("bundles")).exists());
}

#[test]
fn a_scope_makes_the_new_bundle_fire_and_needs_no_item_question() {
    let plan = "bundles: [{name: rust-dev, when: [lang-rust], files: {AGENTS.md: x}}]\n\
                scope: {content: [{id: rust, match: {glob: Cargo.toml}, tags: [lang-rust]}]}\n";
    let s = Setup::new(CONFIG, plan);
    let mut script = Script::new(&[true, true]);
    let (result, out) = s.run(&mut script);
    assert!(result.unwrap(), "{out}");
    assert_eq!(
        script.asked,
        ["Apply this plan?", "Add these lines to config.yaml?"]
    );
    let config: serde_yaml::Value = serde_yaml::from_str(&s.config()).unwrap();
    assert_eq!(
        config["scope"]["content"][0]["tags"][0],
        serde_yaml::Value::from("lang-rust")
    );
}
