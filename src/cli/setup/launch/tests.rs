//! Tests for the engine launch. The launch itself needs an engine binary, so the tests
//! check the command each engine gets and the exit handling.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test fails loudly when its setup fails"
)]

use std::ffi::OsStr;
use std::process::Command;

use super::{claude_command, crush_command, ensure_success, launch_engine};

fn args_of(cmd: &Command) -> Vec<&OsStr> {
    cmd.get_args().collect()
}

#[test]
fn claude_runs_the_skill_as_its_print_prompt() {
    let cmd = claude_command(Command::new("claude"), "do the setup");
    assert_eq!(
        args_of(&cmd),
        [OsStr::new("-p"), OsStr::new("do the setup")]
    );
}

#[test]
fn crush_runs_the_setup_skill_quietly_and_reads_the_skill_from_stdin() {
    let cmd = crush_command(Command::new("crush"));
    assert_eq!(
        args_of(&cmd),
        [
            OsStr::new("run"),
            OsStr::new("--quiet"),
            OsStr::new("Execute the setup-llmenv skill"),
        ]
    );
}

#[test]
fn a_successful_exit_is_ok() {
    let status = Command::new("sh").args(["-c", "exit 0"]).status().unwrap();
    assert!(ensure_success("claude", status).is_ok());
}

#[test]
fn a_failed_exit_names_the_engine_and_its_status() {
    let status = Command::new("sh").args(["-c", "exit 3"]).status().unwrap();
    let err = ensure_success("crush", status).expect_err("a failed exit must be an error");
    let text = err.to_string();
    assert!(text.starts_with("crush exited with status"), "{text}");
    assert!(text.contains('3'), "{text}");
}

#[test]
fn an_unknown_engine_is_rejected_by_name() {
    let err = launch_engine("cursor", "skill").expect_err("an unknown engine must fail");
    assert_eq!(err.to_string(), "unsupported engine: cursor");
}
