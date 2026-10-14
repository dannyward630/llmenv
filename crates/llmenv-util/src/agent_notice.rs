//! Notices for the agent, carried by hook output.
//!
//! The engine drops a hook's stderr, so a log line reaches no one. A failure that the agent must
//! see is recorded here, and the hook output carries every notice of its process (#2658).

use std::sync::{Mutex, PoisonError};

/// Notices raised in this process, in the order they were raised.
static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Add a notice for the agent. A notice that is already recorded is not added again, so a
/// failure that repeats in one process does not repeat its text.
pub fn record(notice: impl Into<String>) {
    let notice = notice.into();
    let mut notices = NOTICES.lock().unwrap_or_else(PoisonError::into_inner);
    if !notices.contains(&notice) {
        notices.push(notice);
    }
}

/// Every notice of this process, one per line, or `None` when there is none. The list stays set,
/// so every call returns the same notices.
pub fn current() -> Option<String> {
    let notices = NOTICES.lock().unwrap_or_else(PoisonError::into_inner);
    (!notices.is_empty()).then(|| notices.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{current, record};

    #[test]
    fn a_repeated_notice_is_kept_once() {
        record("agent_notice test: repeated line");
        record("agent_notice test: repeated line");
        let text = current().unwrap_or_default();
        assert_eq!(text.matches("agent_notice test: repeated line").count(), 1);
    }

    #[test]
    fn notices_keep_the_order_they_were_raised_in() {
        record("agent_notice test: first line");
        record("agent_notice test: second line");
        let text = current().unwrap_or_default();
        let first = text.find("agent_notice test: first line");
        let second = text.find("agent_notice test: second line");
        assert!(
            first.is_some() && second.is_some(),
            "both notices must be present: {text}"
        );
        assert!(
            first < second,
            "the first raised notice must come first: {text}"
        );
    }
}
