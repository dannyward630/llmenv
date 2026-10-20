//! The `gh` program and the deadline of the PR lookup.

use std::time::Duration;

/// How long to wait for `gh pr view` before giving up and degrading to no PR.
/// `gh` hits the GitHub API over the network; a stalled connection must not
/// hang the statusline render. 3s is generous for a healthy connection and
/// short enough that a bad one doesn't stall the prompt.
const DEFAULT_GH_PR_TIMEOUT: Duration = Duration::from_secs(3);

/// Which `gh` binary [`gh_pr_view`] invokes and how long it may take.
///
/// The timeout travels with the binary rather than being read from a constant
/// so tests can point at a fake `gh` *and* give it a budget that machine load
/// can't blow: a test asserting "this JSON parses into this `PrInfo`" must not
/// fail because a `/bin/sh` child missed a 3-second production deadline
/// (#1073). Bundling the two also keeps [`derive_pr`] within the
/// positional-parameter limit.
#[derive(Clone, Copy, Debug)]
pub(super) struct GhPrCmd<'a> {
    pub(super) bin: &'a str,
    pub(super) timeout: Duration,
}

impl<'a> GhPrCmd<'a> {
    /// `bin` with the production timeout ([`DEFAULT_GH_PR_TIMEOUT`]).
    pub(super) fn new(bin: &'a str) -> Self {
        Self {
            bin,
            timeout: DEFAULT_GH_PR_TIMEOUT,
        }
    }
}
