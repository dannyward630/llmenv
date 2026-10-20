//! The outcome of recent adaptive recall calls, for `llmenv doctor` (#2919).
//!
//! Each hook adds its recall calls to a ring file under the state folder. Doctor reads that
//! file with no network call, so it can report a memory server that fails often or always.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Calls kept in the ring. With up to six calls a turn, this covers the last 30 or more turns,
/// which is enough for a rate and small enough to rewrite on every hook.
const RING_CAP: usize = 200;
/// Doctor warns when more than this percent of the kept calls failed.
pub(crate) const WARN_FAILED_PERCENT: usize = 20;
/// Doctor reports the server as stuck when this many of the newest calls all failed.
pub(crate) const STUCK_RUN: usize = 10;
const FILE_KEY: &str = "recall_health";

/// One recall call: its Unix time in seconds, and whether it answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Call {
    at: i64,
    ok: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Ring {
    calls: VecDeque<Call>,
}

/// What doctor reports for the kept calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Health {
    pub(crate) calls: usize,
    pub(crate) failed: usize,
    /// The newest [`STUCK_RUN`] calls all failed.
    pub(crate) stuck: bool,
}

impl Health {
    /// More than [`WARN_FAILED_PERCENT`] of the kept calls failed.
    pub(crate) fn failing_often(&self) -> bool {
        self.failed.saturating_mul(100) > self.calls.saturating_mul(WARN_FAILED_PERCENT)
    }
}

/// The ring file under `state_dir`.
pub(crate) fn path(state_dir: &Path) -> PathBuf {
    state_dir.join(format!("{FILE_KEY}.json"))
}

/// Adds `ok` answered calls and `failed` failed calls at `now` to the ring under `state_dir`.
/// The failures go first, so a hook in which any call answered does not end the ring with a
/// failure. A lock, read, or write that fails is logged and told to the agent, and the calls
/// are not kept.
pub(crate) fn record(state_dir: &Path, ok: usize, failed: usize, now: i64) {
    if ok == 0 && failed == 0 {
        return;
    }
    let path = path(state_dir);
    let Some(_lock) = super::session_state::lock_state_file(state_dir, FILE_KEY, "recall health")
    else {
        note_unsaved(&path, "the lock is busy or cannot be made");
        return;
    };
    let result = read_ring(&path).and_then(|mut ring| {
        let new = std::iter::repeat_n(false, failed).chain(std::iter::repeat_n(true, ok));
        ring.calls.extend(new.map(|ok| Call { at: now, ok }));
        let excess = ring.calls.len().saturating_sub(RING_CAP);
        ring.calls.drain(..excess);
        let bytes = serde_json::to_vec(&ring).context("encoding the ring")?;
        crate::paths::write_owner_only_atomic(&path, &bytes)
            .with_context(|| format!("writing {}", path.display()))
    });
    if let Err(e) = result {
        note_unsaved(&path, &format!("{e:#}"));
    }
}

/// The health of the calls kept under `state_dir`, or `None` when no hook kept a call yet.
///
/// # Errors
/// The file exists but cannot be read, or it is not a valid ring.
pub(crate) fn health(state_dir: &Path) -> anyhow::Result<Option<Health>> {
    let path = path(state_dir);
    let text = match crate::paths::bounded::state_text_io(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let ring: Ring = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a valid recall health file", path.display()))?;
    Ok(Some(assess(&ring.calls)))
}

/// The stored ring. A missing file is an empty ring. A corrupt file is logged and replaced,
/// because the rate of new calls is worth more than a ring nobody can read.
fn read_ring(path: &Path) -> anyhow::Result<Ring> {
    match crate::paths::bounded::state_text_io(path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::error!(
                "recall health {} is corrupt, starting fresh: {e}",
                path.display()
            );
            Ring::default()
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ring::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// The health of `calls`, oldest first.
fn assess(calls: &VecDeque<Call>) -> Health {
    let failed = calls.iter().filter(|c| !c.ok).count();
    let newest_failed = calls.iter().rev().take_while(|c| !c.ok).count();
    Health {
        calls: calls.len(),
        failed,
        stuck: newest_failed >= STUCK_RUN,
    }
}

fn note_unsaved(path: &Path, why: &str) {
    crate::agent_notice::record_logged(format!(
        "llmenv: the recall health file {} was not updated ({why}), so `llmenv doctor` can show \
         an old failure rate. Fix: run `llmenv doctor` to check the state directory.",
        path.display()
    ));
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn no_file_is_no_health() {
        let dir = dir();
        let log = dir.path();
        assert_eq!(health(log).unwrap(), None);
    }

    #[test]
    fn a_recorded_hook_is_counted() {
        let dir = dir();
        let log = dir.path();
        record(log, 4, 1, 100);
        let h = health(log).unwrap().unwrap();
        assert_eq!((h.calls, h.failed), (5, 1));
        assert!(!h.stuck);
        assert!(!h.failing_often(), "1 of 5 is 20%, not more");
        record(log, 0, 0, 200);
        assert_eq!(health(log).unwrap().unwrap().calls, 5, "no call, no entry");
    }

    #[test]
    fn the_ring_keeps_the_newest_calls() {
        let dir = dir();
        let log = dir.path();
        record(log, RING_CAP, 0, 1);
        record(log, 0, STUCK_RUN, 2);
        let h = health(log).unwrap().unwrap();
        assert_eq!((h.calls, h.failed), (RING_CAP, STUCK_RUN));
        assert!(h.stuck);
    }

    #[test]
    fn an_answer_in_the_newest_hook_is_not_stuck() {
        let dir = dir();
        let log = dir.path();
        record(log, 1, STUCK_RUN, 1);
        assert!(!health(log).unwrap().unwrap().stuck);
    }

    #[test]
    fn a_corrupt_file_is_an_error_for_doctor_and_replaced_by_a_hook() {
        let dir = dir();
        let log = dir.path();
        std::fs::write(path(log), "not json").unwrap();
        let err = health(log).unwrap_err();
        assert!(
            format!("{err:#}").contains("not a valid recall health file"),
            "{err:#}"
        );
        record(log, 1, 0, 5);
        assert_eq!(health(log).unwrap().unwrap().calls, 1);
    }

    #[test]
    fn an_unwritable_folder_tells_the_agent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-folder");
        std::fs::write(&file, "").unwrap();
        record(&file, 1, 0, 1);
        let notice = crate::agent_notice::current().unwrap_or_default();
        assert!(notice.contains("recall health file"), "{notice}");
    }

    proptest! {
        /// The warning and the stuck flag follow their definitions for any ring.
        #[test]
        fn assess_matches_the_definitions(oks in prop::collection::vec(any::<bool>(), 0..60)) {
            let calls: VecDeque<Call> = oks.iter().map(|&ok| Call { at: 1, ok }).collect();
            let h = assess(&calls);
            let failed = oks.iter().filter(|ok| !**ok).count();
            prop_assert_eq!(h.failed, failed);
            prop_assert_eq!(h.failing_often(), failed * 5 > oks.len());
            let tail_failed = oks.len() >= STUCK_RUN
                && oks[oks.len() - STUCK_RUN..].iter().all(|ok| !ok);
            prop_assert_eq!(h.stuck, tail_failed);
        }
    }
}
