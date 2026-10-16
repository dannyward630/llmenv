//! The link from a llmenv session to its ICM transcript (#2667).
//!
//! The session map records the link. A transcript starts only when the map can record it, so
//! no transcript is left without a link.

use tracing::error;

use crate::config::SessionLog;
use crate::hook_run::mcp_client::McpHttpClient;
use crate::hook_run::notice;
use crate::session_log::dispatch as transcript_dispatch;
use crate::session_log::{ScopeContext, scope_metadata_json, state};
use llmenv_config::LogLevel;

/// Reuse a previously-recorded transcript session for `csid`, or — when
/// `cfg.transcript` and a client is available — start a new one and persist
/// the correlation. Returns `None` when transcript logging is unavailable and
/// nothing was recorded before.
///
/// `verify`: whether to revalidate a cached id against ICM before trusting it
/// (#1090) rather than treating its presence alone as proof of liveness — the
/// #1085 failure mode (a wrong cached value trusted forever because the only
/// check that would notice consulted the record itself). Revalidation costs a
/// full-transcript fetch (`icm_transcript_show` has no cheap existence-only
/// form), so only the `SessionStart` caller — once per launch — passes
/// `true`; the per-event caller in `run_session_log` passes `false` to avoid
/// turning every logged tool call into an ICM round trip that grows with the
/// transcript itself.
pub(super) async fn ensure_transcript_session(
    cfg: &SessionLog,
    client: Option<&McpHttpClient>,
    csid: &str,
    ctx: &ScopeContext,
    state_path: Option<&std::path::Path>,
    verify: bool,
) -> Option<String> {
    let path = state_path?;
    let existing = match state::lookup_session_at(path, csid) {
        Ok(existing) => existing,
        Err(e) => {
            // Starting a new transcript here would orphan it: the map cannot record the link (#2658).
            crate::agent_notice::record(notice::transcript_unlinked_notice(&e));
            return None;
        }
    };
    if let Some(existing) = existing
        && let Some(id) = cached_transcript_id(client, &existing, verify).await
    {
        return Some(id);
    }
    if !cfg.transcript_wants(LogLevel::Info) {
        return None;
    }
    let Some(client) = client else {
        error!("no ICM transcript started: transcripts are on, but no MCP client is configured");
        return None;
    };
    // Check the map before ICM starts a transcript. A transcript that cannot be linked is an
    // orphan, and each later event would add another one (#2667).
    if let Err(e) = state::check_writable_at(path) {
        error!(error = %e, "no ICM transcript started: the session map cannot be written");
        return None;
    }
    let metadata = scope_metadata_json(ctx);
    let id = match transcript_dispatch::start_session(
        client,
        &ctx.adapter,
        ctx.project.as_deref(),
        &metadata,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            error!(error = %e, "failed to start ICM transcript session");
            return None;
        }
    };
    if let Err(e) = state::record_session_at(path, csid, &id) {
        error!(
            error = %e,
            session_id = %id,
            "ICM transcript started but its link was not recorded, so this event is not logged"
        );
        return None;
    }
    Some(id)
}

/// Decide whether a recorded transcript id still serves this session. `Some` keeps the link.
/// `None` means ICM confirmed the session is gone, so the caller starts a new one.
///
/// A transport failure keeps the link. ICM being unreachable says nothing about the session,
/// and a new transcript would orphan the old one (#2667).
async fn cached_transcript_id(
    client: Option<&McpHttpClient>,
    existing: &str,
    verify: bool,
) -> Option<String> {
    let Some(client) = client.filter(|_| verify) else {
        return Some(existing.to_owned());
    };
    match transcript_dispatch::verify_session(client, existing).await {
        Ok(transcript_dispatch::Liveness::Live) => Some(existing.to_owned()),
        Ok(transcript_dispatch::Liveness::Gone) => {
            error!(
                session_id = %existing,
                "cached ICM transcript session no longer exists, re-establishing"
            );
            None
        }
        Err(e) => {
            error!(
                error = %e,
                session_id = %existing,
                "cannot verify the cached ICM transcript session, keeping its link"
            );
            Some(existing.to_owned())
        }
    }
}
