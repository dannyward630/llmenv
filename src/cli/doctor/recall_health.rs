//! Doctor's `Memory recall:` section (#2919). It reads the ring of recent adaptive recall calls
//! that the hooks keep, so it makes no network call.

use std::path::Path;

use super::{CheckLevel, print_check};
use crate::hook_run::recall_health::{self, Health, STUCK_RUN, WARN_FAILED_PERCENT};

fn health_line(read: anyhow::Result<Option<Health>>, path: &Path) -> (CheckLevel, String) {
    match read {
        Ok(None) => (
            CheckLevel::Info,
            "no adaptive recall calls recorded yet".to_string(),
        ),
        Ok(Some(h)) if h.stuck => (
            CheckLevel::Warn,
            format!(
                "memory recall is stuck: the last {STUCK_RUN} calls all failed ({} of {} recent \
                 calls failed). Fix: check the memory server above, then start a new session.",
                h.failed, h.calls
            ),
        ),
        Ok(Some(h)) if h.failing_often() => (
            CheckLevel::Warn,
            format!(
                "memory recall fails often: {} of {} recent calls failed, more than \
                 {WARN_FAILED_PERCENT}%. Fix: check the memory server above.",
                h.failed, h.calls
            ),
        ),
        Ok(Some(h)) => (
            CheckLevel::Pass,
            format!(
                "memory recall: {} of {} recent calls failed",
                h.failed, h.calls
            ),
        ),
        Err(e) => (
            CheckLevel::Warn,
            format!(
                "cannot read the recall health file {}: {e:#}. Fix: delete the file, and the \
                 next hook writes a new one.",
                path.display()
            ),
        ),
    }
}

pub(super) fn run_doctor_recall_health(use_color: bool, state_dir: &Path) {
    let pass = super::super::doctor_pass(use_color);
    let warn = super::super::doctor_warning(use_color);
    let info = super::super::doctor_info(use_color);
    eprintln!();
    eprintln!("Memory recall:");
    let line = health_line(
        recall_health::health(state_dir),
        &recall_health::path(state_dir),
    );
    print_check(line, &pass, &warn, &info);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(read: anyhow::Result<Option<Health>>) -> (CheckLevel, String) {
        health_line(read, Path::new("/s/recall_health.json"))
    }

    fn health(calls: usize, failed: usize, stuck: bool) -> Health {
        Health {
            calls,
            failed,
            stuck,
        }
    }

    #[test]
    fn no_file_is_info() {
        assert_eq!(line(Ok(None)).0, CheckLevel::Info);
    }

    #[test]
    fn a_low_rate_passes_and_a_high_rate_warns() {
        let (level, text) = line(Ok(Some(health(10, 2, false))));
        assert_eq!(level, CheckLevel::Pass, "{text}");
        let (level, text) = line(Ok(Some(health(10, 3, false))));
        assert_eq!(level, CheckLevel::Warn);
        assert!(text.contains("3 of 10") && text.contains("Fix:"), "{text}");
    }

    #[test]
    fn stuck_wins_over_the_rate() {
        let (level, text) = line(Ok(Some(health(200, 10, true))));
        assert_eq!(level, CheckLevel::Warn);
        assert!(text.contains("stuck"), "{text}");
    }

    #[test]
    fn an_unreadable_file_warns_with_its_path() {
        let (level, text) = line(Err(anyhow::anyhow!("bad json")));
        assert_eq!(level, CheckLevel::Warn);
        assert!(
            text.contains("/s/recall_health.json") && text.contains("bad json"),
            "{text}"
        );
    }
}
