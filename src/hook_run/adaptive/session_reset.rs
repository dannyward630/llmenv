//! The session-start checks for a context reset and for a ledger update that did not save
//! (#2766, #2767).

use super::{RECALL_STATE_UNSAVED, RESET_UNDONE};

/// Whether this `SessionStart` source starts a new context, so the ledger forgets what it sent.
pub(super) fn resets_ledger(source: Option<&str>) -> bool {
    super::super::session_state::context_was_lost(source)
}

/// The notice for the first session-start ledger update, or `None` when it saved. The first
/// update is the only write that carries this session's reset and project, so a failure here
/// is noticed even when the second update later succeeds (#2767).
pub(super) fn first_update_notice(saved: bool, reset: bool) -> Option<&'static str> {
    if saved {
        None
    } else if reset {
        Some(RESET_UNDONE)
    } else {
        Some(RECALL_STATE_UNSAVED)
    }
}
