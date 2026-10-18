use super::*;

#[test]
fn a_task_done_earlier_in_the_same_command_does_not_pass_the_commit_gate() {
    let dir = TempDir::new().unwrap();
    let session = open_session(dir.path());
    let slug = add(dir.path(), &session, "Step one");
    start_task(dir.path(), &slug, false).unwrap();
    let tracker = TaskTracker::default();
    let chained = bash(&format!("llmenv task done {slug} && git commit -m x"));
    let denied = handle_pre_tool_use(&tracker, &chained, Some("s1"), dir.path());
    assert!(
        denied.starts_with("__DENY__:"),
        "the commit gate must deny; got {} bytes",
        denied.len()
    );
    // The commit ran before the task ended, so the task is still in progress for it.
    let before = bash(&format!("git commit -m x && llmenv task done {slug}"));
    assert_eq!(
        handle_pre_tool_use(&tracker, &before, Some("s1"), dir.path()),
        ""
    );
}

#[test]
fn a_task_waited_on_earlier_in_the_same_command_does_not_pass_the_commit_gate() {
    let dir = TempDir::new().unwrap();
    let session = open_session(dir.path());
    let slug = add(dir.path(), &session, "Step one");
    start_task(dir.path(), &slug, false).unwrap();
    let waited = bash(&format!(
        "llmenv task wait {slug} \"needs an answer\" && git commit -m x"
    ));
    let denied = handle_pre_tool_use(&TaskTracker::default(), &waited, Some("s1"), dir.path());
    assert!(
        denied.starts_with("__DENY__:"),
        "the commit gate must deny; got {} bytes",
        denied.len()
    );
}

#[test]
fn commit_gate_names_the_nesting_limit_for_a_command_too_deep_to_split() {
    let dir = TempDir::new().unwrap();
    let _session = open_session(dir.path());
    let nested = format!("{}git commit -m x{}", "$(".repeat(40), ")".repeat(40));
    let denied = handle_pre_tool_use(
        &TaskTracker::default(),
        &bash(&nested),
        Some("s1"),
        dir.path(),
    );
    assert!(
        denied.starts_with("__DENY__:"),
        "the commit gate must deny; got {} bytes",
        denied.len()
    );
    assert!(
        denied.contains("32 levels deep"),
        "the deny must name the 32-level limit; got {} bytes",
        denied.len()
    );
    assert!(
        denied.contains("Split the command"),
        "the deny must say to split the command; got {} bytes",
        denied.len()
    );
    assert!(
        !denied.contains("no task is in progress"),
        "the deny must not blame a missing task; got {} bytes",
        denied.len()
    );
}

#[test]
fn a_nested_commit_stays_denied_on_retry() {
    let dir = TempDir::new().unwrap();
    let _session = open_session(dir.path());
    let nested = format!("{}git commit -m x{}", "$(".repeat(40), ")".repeat(40));
    let tracker = TaskTracker::default();
    let first = handle_pre_tool_use(&tracker, &bash(&nested), Some("s1"), dir.path());
    let retry = handle_pre_tool_use(&tracker, &bash(&nested), Some("s1"), dir.path());
    assert!(
        first.starts_with("__DENY__:"),
        "the first attempt must deny; got {} bytes",
        first.len()
    );
    assert!(
        retry.starts_with("__DENY__:"),
        "the retry must deny; got {} bytes",
        retry.len()
    );
}

#[test]
fn a_task_started_and_finished_in_one_command_re_arms_the_start_reminder() {
    let dir = TempDir::new().unwrap();
    let session = open_session(dir.path());
    let one = add(dir.path(), &session, "Step one");
    add(dir.path(), &session, "Step two");
    let tracker = TaskTracker::default();
    let edit = serde_json::json!({ "tool_name": "Edit" });
    let post = || handle_post_tool_use(&tracker, &edit, Some("s1"), dir.path());
    assert!(post().contains("llmenv task start step-one"));
    // Both changes land between two tool calls, so the next edit sees step two queued.
    start_task(dir.path(), &one, false).unwrap();
    done_task(dir.path(), &one).unwrap();
    assert!(post().contains("llmenv task start step-two"));
}

#[test]
fn another_task_ending_before_the_commit_does_not_end_the_task_in_progress() {
    assert!(!ends_task_before_commit(
        "llmenv task done other && git commit -m x",
        "step-one"
    ));
    assert!(ends_task_before_commit(
        "llmenv task done step-one && git commit -m x",
        "step-one"
    ));
}

#[test]
fn a_commit_in_a_substitution_before_the_task_end_does_not_pass_the_gate() {
    assert!(!ends_task_before_commit(
        "echo $(git commit -m x); llmenv task done step-one",
        "step-one"
    ));
    assert!(ends_task_before_commit(
        "llmenv task done step-one; echo $(git commit -m x)",
        "step-one"
    ));
}

#[test]
fn a_task_end_in_a_commit_argument_substitution_passes_the_gate() {
    // Bash runs `llmenv task done` before the commit that holds it (#2544).
    assert!(ends_task_before_commit(
        "git commit -m \"$(llmenv task done step-one)\"",
        "step-one"
    ));
}

#[test]
fn substitutions_nested_past_the_bound_fail_closed() {
    let nested = |n: usize| format!("{}a{}", "$(".repeat(n), ")".repeat(n));
    assert!(split_substitutions(&nested(MAX_SUBSTITUTION_DEPTH)).is_ok());
    let too_deep = nested(MAX_SUBSTITUTION_DEPTH + 1);
    assert_eq!(split_substitutions(&too_deep), Err(NestingTooDeep));
    assert!(shell_segments(&too_deep).is_err());
    // An unsplittable command may end the task, so the gate must deny the commit (fail closed).
    assert!(ends_task_before_commit(&too_deep, "step-one"));
    assert!(runs_commit_or_pr(&too_deep));
}

#[test]
fn process_and_backtick_nesting_are_bounded_too() {
    let process = |n: usize| format!("{}a{}", "<(".repeat(n), ")".repeat(n));
    assert!(split_substitutions(&process(MAX_SUBSTITUTION_DEPTH)).is_ok());
    assert_eq!(
        split_substitutions(&process(MAX_SUBSTITUTION_DEPTH + 1)),
        Err(NestingTooDeep)
    );
    // A backtick is one level, and the `$(` inside it adds one more per level.
    let backtick = |n: usize| format!("`{}a{}`", "$(".repeat(n), ")".repeat(n));
    assert!(split_substitutions(&backtick(MAX_SUBSTITUTION_DEPTH - 1)).is_ok());
    assert_eq!(
        split_substitutions(&backtick(MAX_SUBSTITUTION_DEPTH)),
        Err(NestingTooDeep)
    );
}

#[test]
fn a_too_deep_task_end_before_a_commit_is_denied_by_the_gate() {
    let nested = format!(
        "{}a{}",
        "$(".repeat(MAX_SUBSTITUTION_DEPTH + 1),
        ")".repeat(MAX_SUBSTITUTION_DEPTH + 1)
    );
    let command = format!("{nested}; llmenv task done step-one; git commit -m x");
    assert!(ends_task_before_commit(&command, "step-one"));
}

#[test]
fn a_quoted_close_paren_does_not_end_a_substitution() {
    // Bash reads the `)` inside the double quotes as text, so the commit after the
    // substitution is a real segment (#2544 follow-up).
    let command = r#"echo "$(echo ")"; git commit -m x)""#;
    assert!(runs_commit_or_pr(command));
    assert!(!ends_task_before_commit(command, "step-one"));
}

#[test]
fn a_process_substitution_task_end_does_not_pass_before_the_commit() {
    // Bash runs a process substitution alongside its command, not before it.
    assert!(!ends_task_before_commit(
        "git commit -m x > >(llmenv task done step-one)",
        "step-one"
    ));
}
