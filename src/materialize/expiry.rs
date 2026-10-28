//! The age test that decides whether a cache or temp entry is old enough to remove (#2954).

use std::path::Path;
use std::time::{Duration, SystemTime};

/// An mtime this far ahead of the clock is a fresh write, not skew. A coarse file-system
/// timestamp can put a write slightly ahead of the clock read.
const CLOCK_SKEW_TOLERANCE: Duration = Duration::from_secs(5);

/// Check whether `mtime` is older than `older_than` relative to `now`.
/// An mtime in the future is kept and logged. A clock that moved backward must not
/// delete live data, and the `state/tmp` sweep keeps such an entry too (#2860).
///
/// `now` is read once before a walk, so a write made during the walk can be ahead of it.
/// The clock is read again before a future mtime is called skew (#2954).
pub(crate) fn is_expired(
    now: SystemTime,
    mtime: SystemTime,
    older_than: Duration,
    path: &Path,
) -> bool {
    if let Ok(elapsed) = now.duration_since(mtime) {
        return elapsed > older_than;
    }
    match SystemTime::now().duration_since(mtime) {
        Ok(elapsed) => elapsed > older_than,
        Err(e) if e.duration() > CLOCK_SKEW_TOLERANCE => {
            // Error level: an entry with a future mtime is never collected, so the cause must show.
            tracing::error!(
                skew_secs = e.duration().as_secs(),
                "clock skew: {} has an mtime {}s in the future; keeping the entry",
                path.display(),
                e.duration().as_secs()
            );
            false
        }
        Err(_) => false,
    }
}
