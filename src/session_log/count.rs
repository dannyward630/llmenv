//! The record count of a session log, for the status file.

use std::path::Path;

/// The number of records in the session log at `path`, or `None` when it is absent or cannot be
/// read. A failure is logged at debug level, because the count only feeds the status file.
pub(crate) fn count_records(path: &Path) -> Option<u64> {
    // The log can be large, and only its records are counted, so it is streamed. The open does not
    // block on a FIFO.
    let file = match crate::paths::bounded::open_regular(path) {
        Ok(Some(file)) => file,
        Ok(None) => return None,
        Err(e) => {
            tracing::debug!("session log stat collection failed (non-fatal): {e}");
            return None;
        }
    };
    let mut count = 0u64;
    for line in std::io::BufRead::lines(std::io::BufReader::new(file)) {
        match line {
            Ok(line) if !line.trim().is_empty() => count = count.saturating_add(1),
            Ok(_) => {}
            Err(e) => {
                tracing::debug!("session log stat collection failed (non-fatal): {e}");
                return None;
            }
        }
    }
    Some(count)
}
