//! Classifies a failed `gh pr view` run: no PR, a login problem, or a failure.

use serde::{Deserialize, Serialize};

use super::PrLookup;

/// A `gh pr view` call that gave no answer. A non-zero exit for "no open PR" or "no remote" is
/// an answer, not a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::cli::statusline) enum GhFailure {
    /// `gh` is not installed.
    Missing,
    /// `gh` is not logged in, or its login expired.
    Unauthenticated,
    /// A timeout, a failed read or wait, or a spawn error other than a missing binary.
    Failed,
}

impl GhFailure {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Missing => "pr: gh missing",
            Self::Unauthenticated => "pr: gh not logged in",
            Self::Failed => "pr: gh failed",
        }
    }
}

/// The `gh pr view` errors that mean "there is no PR to show", not a failure. Entries are
/// lowercase and match case-insensitively: real `gh` capitalizes the first letter (#2975).
pub(super) const GH_NO_PR_SIGNS: [&str; 3] = [
    "no pull requests found",
    "no git remotes found",
    "not a git repository",
];

/// The `gh` errors that mean the login is missing or expired.
pub(super) const GH_LOGIN_SIGNS: [&str; 3] =
    ["gh auth login", "HTTP 401", "authentication required"];

/// The lookup for a `gh pr view` that exited non-zero. Only a known "no PR" answer is
/// `Ok(None)`. The "no PR" check runs first, because that message quotes the branch name, and a
/// branch name can hold a login word. Any other exit, such as a network or rate-limit error, is
/// a failure, so it cannot look like "no open PR" (#2821).
pub(super) fn classify_gh_exit(status: std::process::ExitStatus, stderr: &str) -> PrLookup {
    let first_line = stderr.lines().next().unwrap_or("");
    let lowered = stderr.to_ascii_lowercase();
    if GH_NO_PR_SIGNS.iter().any(|sign| lowered.contains(sign)) {
        tracing::debug!("gh pr view exited {status}, read as no PR: {first_line}");
        return Ok(None);
    }
    if GH_LOGIN_SIGNS.iter().any(|sign| stderr.contains(sign)) {
        tracing::error!("gh pr view: gh is not logged in ({status}): {first_line}");
        return Err(GhFailure::Unauthenticated);
    }
    tracing::error!("gh pr view failed ({status}): {first_line}");
    Err(GhFailure::Failed)
}
