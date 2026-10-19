//! `handle_stop` tests.

use super::tests::{bash_payload, config};
use super::*;
use tempfile::TempDir;

const WIP_REMINDER: &str = "You still have task(s) in progress:\n- foo\nkeep working";

#[test]
fn empty_reminder_passes_through() {
    let dir = TempDir::new().expect("test");
    let out = handle_stop("", Some("s1"), &config(3), dir.path());
    assert!(out.is_empty());
}

#[test]
fn no_session_id_passes_reminder_through_unchanged() {
    let dir = TempDir::new().expect("test");
    let out = handle_stop(WIP_REMINDER, None, &config(3), dir.path());
    assert_eq!(out, WIP_REMINDER);
}

#[test]
fn below_threshold_reminder_unchanged() {
    let dir = TempDir::new().expect("test");
    for _ in 0..2 {
        let out = handle_stop(WIP_REMINDER, Some("s1"), &config(3), dir.path());
        assert_eq!(
            out, WIP_REMINDER,
            "below threshold must not append anything"
        );
    }
}

#[test]
fn at_threshold_appends_task_wait_pointer() {
    let dir = TempDir::new().expect("test");
    for _ in 0..2 {
        handle_stop(WIP_REMINDER, Some("s1"), &config(3), dir.path());
    }
    let out = handle_stop(WIP_REMINDER, Some("s1"), &config(3), dir.path());
    assert!(
        out.starts_with(WIP_REMINDER),
        "original reminder must still be present"
    );
    assert!(
        out.contains("llmenv task wait"),
        "must point at the escape hatch: {out}"
    );
}

#[test]
fn different_reminder_resets_stop_streak() {
    let dir = TempDir::new().expect("test");
    handle_stop(WIP_REMINDER, Some("s1"), &config(2), dir.path());
    let other = "You still have task(s) in progress:\n- bar\nkeep working";
    let out = handle_stop(other, Some("s1"), &config(2), dir.path());
    assert_eq!(out, other, "a different reminder must reset the streak");
}

// #1247: the escalated message (task_wait pointer) is only useful advice
// up to a point — past SILENCE_AFTER_THRESHOLD_MULTIPLE repeats of the
// *same* threshold, the reminder must stop firing entirely rather than
// looping the same "keep working" imperative forever.
#[test]
fn goes_silent_after_threshold_multiple_repeats() {
    let dir = TempDir::new().expect("test");
    let threshold = 2;
    let silence_at = threshold * SILENCE_AFTER_THRESHOLD_MULTIPLE;
    for _ in 0..silence_at - 1 {
        let out = handle_stop(WIP_REMINDER, Some("s1"), &config(threshold), dir.path());
        assert!(!out.is_empty(), "must still remind below the silence point");
    }
    let out = handle_stop(WIP_REMINDER, Some("s1"), &config(threshold), dir.path());
    assert!(
        out.is_empty(),
        "must go silent at the silence point, got: {out:?}"
    );
    // Confirm it stays silent, not a one-turn blip.
    let out = handle_stop(WIP_REMINDER, Some("s1"), &config(threshold), dir.path());
    assert!(out.is_empty(), "must stay silent past the silence point");
}

// #1247: silence must not be permanent — a changed task set (a
// different reminder string) re-arms the normal escalation from scratch.
#[test]
fn a_new_reminder_after_silence_is_shown_again() {
    let dir = TempDir::new().expect("test");
    let threshold = 2;
    let silence_at = threshold * SILENCE_AFTER_THRESHOLD_MULTIPLE;
    for _ in 0..silence_at {
        handle_stop(WIP_REMINDER, Some("s1"), &config(threshold), dir.path());
    }
    let other = "You still have task(s) in progress:\n- bar\nkeep working";
    let out = handle_stop(other, Some("s1"), &config(threshold), dir.path());
    assert_eq!(
        out, other,
        "a changed task set must be shown again, not silenced"
    );
}

#[test]
fn pre_tool_use_streak_and_stop_streak_are_independent() {
    let dir = TempDir::new().expect("test");
    // Drive the PreToolUse tracker to just below its own threshold...
    handle_pre_tool_use(&bash_payload("ls"), Some("s1"), &config(2), dir.path());
    // ...then a Stop event with a fresh reminder must not be affected by
    // (or reset) the PreToolUse streak, and vice versa.
    let out = handle_stop(WIP_REMINDER, Some("s1"), &config(2), dir.path());
    assert_eq!(out, WIP_REMINDER);
    let tool_out = handle_pre_tool_use(&bash_payload("ls"), Some("s1"), &config(2), dir.path());
    assert!(
        !tool_out.is_empty(),
        "PreToolUse streak must have kept counting: {tool_out}"
    );
}
