//! Notices for the agent, carried by hook output.
//!
//! The engine drops a hook's stderr, so a log line reaches no one. A failure that the agent must
//! see is recorded here, and the hook output carries every notice of its process (#2658).

use std::sync::{Mutex, PoisonError};

/// The notices of one process, in the order they were first raised.
#[derive(Debug, Default)]
pub(crate) struct NoticeList {
    notices: Vec<String>,
}

impl NoticeList {
    /// An empty list. `const` so the process-wide list can be a `static`.
    pub(crate) const fn new() -> Self {
        Self {
            notices: Vec::new(),
        }
    }

    /// Add a notice. A notice that is already present is not added again, so a failure that
    /// repeats does not repeat its text. Line breaks become spaces, because `current` joins
    /// notices with newlines and one notice must stay one line.
    pub(crate) fn record(&mut self, notice: impl Into<String>) {
        let notice = notice.into().replace(['\n', '\r'], " ");
        if !self.notices.contains(&notice) {
            self.notices.push(notice);
        }
    }

    /// Every notice, one per line, or `None` when there is none.
    pub(crate) fn current(&self) -> Option<String> {
        (!self.notices.is_empty()).then(|| self.notices.join("\n"))
    }
}

/// Notices raised in this process.
static NOTICES: Mutex<NoticeList> = Mutex::new(NoticeList::new());

/// Add a notice for the agent. See `NoticeList::record`.
pub(crate) fn record(notice: impl Into<String>) {
    NOTICES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .record(notice);
}

/// Every notice of this process, one per line, or `None` when there is none. The list stays set,
/// so every call returns the same notices.
pub(crate) fn current() -> Option<String> {
    NOTICES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .current()
}

#[cfg(test)]
mod tests {
    use super::{NoticeList, current, record};
    use proptest::prelude::*;

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

    #[test]
    fn an_empty_list_has_no_current_text() {
        assert_eq!(NoticeList::new().current(), None);
    }

    #[test]
    fn a_notice_with_a_line_break_stays_one_line() {
        let mut list = NoticeList::new();
        list.record("first\nsecond\r\nthird");
        assert_eq!(list.current().as_deref(), Some("first second  third"));
    }

    proptest! {
        #[test]
        fn current_is_the_first_occurrence_dedupe_joined_by_newlines(
            raised in proptest::collection::vec("[a-z \n\r]{0,12}", 0..16),
        ) {
            let mut list = NoticeList::new();
            let mut expected: Vec<String> = Vec::new();
            for notice in &raised {
                list.record(notice.as_str());
                let flat = notice.replace(['\n', '\r'], " ");
                if !expected.contains(&flat) {
                    expected.push(flat);
                }
            }
            let want = (!expected.is_empty()).then(|| expected.join("\n"));
            prop_assert_eq!(list.current(), want);
        }
    }
}
