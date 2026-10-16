//! The notices that a `SessionStart` event adds for the agent (#2815).
//!
//! The hint about stale `state/tmp` entries is shown once per UTC day. The day is kept in a
//! marker file in the state directory, so a later session on the same day stays quiet.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::materialize::tmp_sweep::{self, StaleSummary};

use super::mcp_health;
use super::notice::problem_notice;

/// The marker file that holds the UTC day of the last stale-entry hint.
const HINT_MARKER: &str = "stale-tmp-hint.day";

/// The retention that the cache sweep uses when the config leaves it unset (#2814).
const DEFAULT_RETENTION_HOURS: u64 = 168;

const KIB: u64 = 1 << 10;
const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// The notices for a `SessionStart` event: the MCP health line, the cache roots line, and the
/// stale `state/tmp` hint. Each part is joined in the order above.
pub(super) fn session_start_notices(
    rt: Option<&tokio::runtime::Runtime>,
    config: &crate::config::Config,
    config_dir: &Path,
    active: &crate::scope::ActiveScopes,
    roots_notice: Option<String>,
) -> Option<String> {
    let health = rt.and_then(|rt| mcp_health::session_start_notice(rt, config, config_dir, active));
    let base = match (health, roots_notice) {
        (Some(h), Some(r)) => Some(format!("{h}{r}")),
        (h, r) => h.or(r),
    };
    let retention = config
        .cache
        .cache_retention_hours
        .unwrap_or(DEFAULT_RETENTION_HOURS);
    let hint = match crate::paths::state_dir() {
        Ok(state_dir) => stale_tmp_hint(&state_dir, retention, SystemTime::now()),
        Err(e) => Some(problem_notice(
            &format!("the stale state/tmp check did not run: {e:#}"),
            "run `llmenv doctor` to check the state directory",
        )),
    };
    match (base, hint) {
        (Some(b), Some(h)) => Some(format!("{b}\n{h}")),
        (b, h) => b.or(h),
    }
}

/// The hint for stale `state/tmp` entries, or `None` when today's hint is already shown, or
/// when nothing is stale. A failed check returns a notice, so the problem reaches the agent.
fn stale_tmp_hint(state_dir: &Path, hours: u64, now: SystemTime) -> Option<String> {
    let day = utc_day(now);
    let marker = state_dir.join(HINT_MARKER);
    match std::fs::read_to_string(&marker) {
        Ok(text) if text.trim() == day.to_string() => return None,
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::error!(path = %marker.display(), error = %e, "cannot read the hint marker");
        }
    }
    let older_than = Duration::from_secs(hours.saturating_mul(3600));
    let summary = match tmp_sweep::stale_summary(state_dir, older_than) {
        Ok(summary) => summary,
        Err(e) => {
            return Some(problem_notice(
                &format!("the stale state/tmp check failed: {e:#}"),
                "run `llmenv doctor` to check the state directory",
            ));
        }
    };
    if let Err(e) = std::fs::write(&marker, day.to_string()) {
        tracing::error!(path = %marker.display(), error = %e, "cannot write the hint marker");
    }
    (summary.count > 0).then(|| stale_tmp_text(&summary, hours))
}

/// The one-line hint. It names the count, the size, the retention, and the command.
fn stale_tmp_text(summary: &StaleSummary, hours: u64) -> String {
    format!(
        "llmenv: {} state/tmp entries ({}) have not changed in {hours} hours. \
         Fix: run `llmenv doctor --gc` to remove them.",
        summary.count,
        human_bytes(summary.bytes)
    )
}

/// The whole UTC days since the Unix epoch. A clock before the epoch counts as day 0.
fn utc_day(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() / 86_400)
}

/// A byte count in the largest unit that keeps the number at least 1. Integer division only,
/// so the text is exact and needs no float cast.
fn human_bytes(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::materialize::tmp_sweep::StaleSummary;

    use super::{human_bytes, stale_tmp_hint, stale_tmp_text, utc_day};

    const DAY: Duration = Duration::from_secs(86_400);

    #[test]
    fn utc_day_counts_whole_days_from_the_epoch() {
        assert_eq!(utc_day(UNIX_EPOCH), 0);
        assert_eq!(utc_day(UNIX_EPOCH + DAY - Duration::from_secs(1)), 0);
        assert_eq!(utc_day(UNIX_EPOCH + DAY), 1);
    }

    #[test]
    fn a_clock_before_the_epoch_counts_as_day_zero() {
        assert_eq!(utc_day(UNIX_EPOCH - DAY), 0);
    }

    #[test]
    fn human_bytes_picks_the_largest_unit_that_keeps_one_or_more() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1024), "1 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 + 5), "3 MiB");
        assert_eq!(human_bytes(6 * 1024 * 1024 * 1024), "6 GiB");
    }

    #[test]
    fn the_hint_names_the_count_size_retention_and_command() {
        let summary = StaleSummary {
            count: 4,
            bytes: 2 * 1024 * 1024,
        };

        let text = stale_tmp_text(&summary, 168);

        assert_eq!(
            text,
            "llmenv: 4 state/tmp entries (2 MiB) have not changed in 168 hours. \
             Fix: run `llmenv doctor --gc` to remove them."
        );
    }

    #[test]
    fn the_hint_is_shown_once_per_day_and_only_when_entries_are_stale() {
        let state = tempfile::tempdir().unwrap();
        let tmp = state.path().join("tmp");
        let stale = tmp.join("cargo-old");
        fs::create_dir_all(&stale).unwrap();
        fs::write(stale.join("artifact"), [0_u8; 64]).unwrap();
        // An entry is stale by its newest mtime, so the file inside is aged along with the folder.
        let long_ago = SystemTime::now() - 30 * DAY;
        for path in [stale.clone(), stale.join("artifact")] {
            fs::File::open(&path)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        let now = SystemTime::now();

        let first = stale_tmp_hint(state.path(), 168, now).expect("a stale entry gives a hint");
        assert!(first.contains("1 state/tmp entries"), "{first}");
        assert!(
            stale_tmp_hint(state.path(), 168, now).is_none(),
            "the hint repeated on the same day"
        );
        assert!(
            stale_tmp_hint(state.path(), 168, now + DAY).is_some(),
            "the hint did not return on the next day"
        );
        assert!(stale.exists(), "the hint removed an entry");
    }

    #[test]
    fn no_hint_when_nothing_is_stale() {
        let state = tempfile::tempdir().unwrap();
        fs::create_dir_all(state.path().join("tmp").join("fresh")).unwrap();

        assert!(stale_tmp_hint(state.path(), 168, SystemTime::now()).is_none());
    }
}
