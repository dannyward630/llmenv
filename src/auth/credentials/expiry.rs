//! Expiry checks for the cached OAuth login token.

use super::{Credentials, now_ms};

impl Credentials {
    /// True when this blob is worthless: the access token has expired *and* no
    /// live refresh token remains. A stale access token with a live refresh
    /// token is still worth caching — Claude Code renews it on next use.
    ///
    /// A blob with no `expiresAt` is treated as live; an unknown expiry is not
    /// grounds for discarding a credential.
    #[must_use]
    pub(super) fn is_expired(&self, now_ms: i64) -> bool {
        // MCP server tokens are siblings of the login token in the same store and
        // expire independently, so a dead login token must not discard them —
        // that would silently log the user out of every third-party MCP server
        // (Slack, Notion, …). MCP entries carry no expiry of their own; Claude
        // Code refreshes or re-authorizes them per server (#1058).
        if self.mcp_server_count() > 0 {
            return false;
        }
        // The access token counts as dead only once its expiry plus the GC skew tolerance has
        // passed (#3098). The tolerance runs one way: a token that expired a moment ago stays live,
        // and a clock that runs behind still reads a dead token as live.
        let skew_ms = i64::try_from(crate::materialize::expiry::CLOCK_SKEW_TOLERANCE.as_millis())
            .unwrap_or(i64::MAX);
        let access_dead = self
            .expires_at()
            .is_some_and(|t| t.saturating_add(skew_ms) < now_ms);
        let refresh_live = self.refresh_expires_at().is_some_and(|t| t > now_ms);
        access_dead && !refresh_live
    }

    /// [`Credentials::is_expired`] against the current wall clock.
    #[must_use]
    pub(crate) fn is_expired_now(&self) -> bool {
        self.is_expired(now_ms())
    }
}
