//! Address and URL checks that gate the outbound requests of an MCP client.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, anyhow};
use url::{Host, Url};

use crate::cleartext::{embedded_ipv4, is_cleartext_safe, is_unique_local_v6};

/// Which address ranges [`validate_url_production`] allows past the SSRF gate.
///
/// The gate is shared by two callers with different trust models: llmenv's own
/// ICM memory/transcript backend (`icm serve`) is *expected* to live on
/// loopback or the operator's LAN (AGENTS.md: "the resolved icm MCP endpoint
/// can be a remote `icm serve`"), while a configured third-party SaaS API
/// (e.g. the `umans` throttle backend) should never legitimately resolve
/// inside a private network — that would itself be a sign of a misconfigured
/// or hijacked endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsrfPolicy {
    /// Reject loopback, private (RFC 1918 / ULA), link-local, unspecified, and
    /// broadcast addresses. For endpoints expected to be public internet
    /// services.
    PublicOnly,
    /// Reject only link-local, unspecified, and broadcast addresses (still
    /// blocks classic SSRF/metadata targets with no legitimate use here).
    /// Loopback and private ranges are allowed — the expected topology for
    /// llmenv's own same-host or LAN-hosted ICM backend.
    AllowPrivateNetwork,
}

/// Report why an IP address is blocked for SSRF safety, or `None` if it is a
/// routable public address.
///
/// Single source of truth shared by literal-IP and resolved-domain validation.
/// IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) are unwrapped and judged by
/// their IPv4 form so a blocked v4 range cannot be smuggled through the v6
/// namespace (#191).
pub(super) fn blocked_reason(ip: &IpAddr, policy: SsrfPolicy) -> Option<&'static str> {
    let block_private = policy == SsrfPolicy::PublicOnly;
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() {
                block_private.then_some("loopback IPv4")
            } else if v4.is_private() {
                block_private.then_some("private IPv4")
            } else if v4.is_link_local() {
                Some("link-local IPv4")
            } else if v4.is_unspecified() {
                Some("unspecified IPv4")
            } else if v4.is_broadcast() {
                Some("broadcast IPv4")
            } else {
                None
            }
        }
        IpAddr::V6(v6) => {
            if let Some(embedded) = embedded_ipv4(v6) {
                return blocked_reason(&IpAddr::V4(embedded), policy);
            }
            if v6.is_loopback() {
                block_private.then_some("loopback IPv6")
            } else if v6.is_unspecified() {
                Some("unspecified IPv6")
            } else if v6.is_unicast_link_local() {
                Some("link-local IPv6")
            } else if is_unique_local_v6(v6) {
                block_private.then_some("unique-local IPv6 (ULA)")
            } else {
                None
            }
        }
    }
}

/// Resolve `(host, port)` to socket addresses, bounded by `timeout`.
///
/// `ToSocketAddrs::to_socket_addrs()` is a blocking syscall with no timeout of
/// its own. Under DNS resolver failure (unreachable nameserver, VPN drop,
/// flaky mDNS) it can hang for minutes — long enough to defeat every caller's
/// own request timeout, since resolution happens before any HTTP client
/// exists to enforce one. Bound it by racing it against `timeout` on a
/// dedicated thread; on timeout the thread is abandoned rather than joined —
/// it dies with the process (these are short-lived CLI invocations), and
/// there is no portable way to cancel a blocked `getaddrinfo()` call.
pub fn resolve_with_timeout(
    host: &str,
    port: u16,
    timeout: Duration,
) -> anyhow::Result<Vec<SocketAddr>> {
    let host_owned = host.to_string();
    let result = run_with_timeout(
        move || {
            (host_owned.as_str(), port)
                .to_socket_addrs()
                .map(|it| it.collect::<Vec<_>>())
        },
        timeout,
    );
    match result {
        Ok(Ok(addrs)) => Ok(addrs),
        Ok(Err(e)) => Err(anyhow!("failed to resolve host '{host}': {e}")),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(anyhow!(
            "DNS resolution for host '{host}' timed out after {timeout:?}"
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(anyhow!("DNS resolution for host '{host}' thread panicked"))
        }
    }
}

/// Race an arbitrary blocking closure against `timeout` on a dedicated thread.
///
/// Split out from `resolve_with_timeout` so the timeout/race mechanism itself
/// can be tested deterministically (a controllable closure) instead of racing
/// against real DNS resolution — the latter timed out or not depending on
/// scheduling noise (thread spawn latency, resolver cache state), not on the
/// mechanism under test.
pub(super) fn run_with_timeout<T, F>(f: F, timeout: Duration) -> Result<T, mpsc::RecvTimeoutError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout)
}

/// The URL without credentials, query string, or fragment, for error text that reaches a
/// terminal.
pub(super) fn redact_url(url: &str) -> String {
    Url::parse(url).map_or_else(
        |_| "(invalid URL)".to_string(),
        |mut u| {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        },
    )
}

/// Refuse `http://` when any vetted address is outside the operator's own network. The
/// remote `icm serve` topology runs on loopback or the operator's LAN, so cleartext stays
/// allowed there (#2476).
pub(super) fn require_https_for_public(url: &str, addrs: &[SocketAddr]) -> anyhow::Result<()> {
    let parsed = Url::parse(url).context("invalid URL")?;
    if parsed.scheme() != "http" {
        return Ok(());
    }
    match addrs.iter().find(|a| !is_cleartext_safe(&a.ip())) {
        Some(addr) => Err(anyhow!(
            "refusing cleartext http:// to {} ({}) for {}: use https://, or move the server \
             onto a loopback, private, or Tailscale address",
            addr.ip(),
            parsed.host_str().unwrap_or("?"),
            redact_url(url)
        )),
        None => Ok(()),
    }
}

/// Drop link-local addresses from a resolved domain under `AllowPrivateNetwork`.
///
/// mDNS names (`host.local`) resolve to a reachable address plus `fe80::` addresses for
/// every interface. The caller pins reqwest to the returned list, so a dropped address is
/// never connected to and the metadata-endpoint protection holds (#2512). `PublicOnly`
/// keeps every address, so the caller rejects on the first blocked one.
///
/// # Errors
/// The host resolved to no addresses, or only to link-local addresses.
pub(super) fn drop_link_local_for_domain(
    resolved: Vec<SocketAddr>,
    policy: SsrfPolicy,
    url: &str,
) -> anyhow::Result<Vec<SocketAddr>> {
    if resolved.is_empty() {
        return Err(anyhow!(
            "host of URL {} resolved to no addresses",
            redact_url(url)
        ));
    }
    if policy != SsrfPolicy::AllowPrivateNetwork {
        return Ok(resolved);
    }
    let (dropped, kept): (Vec<SocketAddr>, Vec<SocketAddr>) =
        resolved.into_iter().partition(|a| is_link_local(&a.ip()));
    match (kept.is_empty(), dropped.first()) {
        (true, Some(first)) => Err(anyhow!(
            "host of URL {} resolved only to link-local address {} (SSRF); \
             no routable address is left",
            redact_url(url),
            first.ip()
        )),
        _ => Ok(kept),
    }
}

/// True for a link-local address, including an IPv4 one wrapped in an IPv6 form.
pub(super) fn is_link_local(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => {
            embedded_ipv4(v6).map_or_else(|| v6.is_unicast_link_local(), |v4| v4.is_link_local())
        }
    }
}

/// Validate ICM backend URL to prevent SSRF attacks and return the host string
/// together with the vetted set of socket addresses the connection may target.
///
/// Rejects unsupported schemes, blocked literal IPs, and — for hostnames —
/// resolves DNS and rejects the URL if *any* resolved address is blocked, except that
/// `AllowPrivateNetwork` first drops link-local addresses of a domain host (#2512). The
/// caller pins reqwest to the returned host→addrs mapping so reqwest cannot
/// re-resolve to an unvetted address at send() time (DNS-rebinding TOCTOU
/// mitigation, #191). The host is returned from the same parse that produced the
/// addresses, so the caller never re-parses the URL — a second parse could
/// disagree about the host and pin the wrong (or no) mapping.
///
/// `policy` decides whether loopback/private ranges are allowed (see
/// [`SsrfPolicy`]); `timeout` bounds DNS resolution for domain hosts, in
/// addition to whatever the caller uses to bound the request itself.
///
/// # Errors
/// Returns an error for an unparseable URL, an unsupported scheme, a missing
/// host, a DNS resolution failure or timeout, or any resolved/literal address
/// blocked under the given `policy`.
pub fn validate_url_production(
    url: &str,
    policy: SsrfPolicy,
    timeout: Duration,
) -> anyhow::Result<(String, Vec<SocketAddr>)> {
    // The URL can carry credentials or a token in its query, so no error below prints it raw.
    let parsed = Url::parse(url).context("invalid URL")?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(anyhow!(
            "unsupported URL scheme '{}' (only http/https allowed)",
            parsed.scheme()
        ));
    }

    let host = parsed
        .host()
        .ok_or_else(|| anyhow!("URL {} has no host", redact_url(url)))?;
    // reqwest's resolve_to_addrs keys on the unbracketed host string; Host's
    // Display matches host_str without the IPv6 brackets, which is what we pin.
    let host_key = match host {
        Host::Ipv4(v4) => v4.to_string(),
        Host::Ipv6(v6) => v6.to_string(),
        Host::Domain(name) => name.to_string(),
    };
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| anyhow!("URL {} has no port and an unknown default", redact_url(url)))?;

    let addrs: Vec<SocketAddr> = match host {
        Host::Ipv4(v4) => vec![SocketAddr::new(IpAddr::V4(v4), port)],
        Host::Ipv6(v6) => vec![SocketAddr::new(IpAddr::V6(v6), port)],
        Host::Domain(name) => {
            let resolved = resolve_with_timeout(name, port, timeout)?;
            drop_link_local_for_domain(resolved, policy, url)?
        }
    };

    // Reject if ANY remaining address is blocked. A permissive "some address is
    // public" rule would let an attacker pair one public A record with a private
    // one and gamble on connection ordering.
    for addr in &addrs {
        if let Some(reason) = blocked_reason(&addr.ip(), policy) {
            return Err(anyhow!("{reason} address {} not allowed (SSRF)", addr.ip()));
        }
    }

    Ok((host_key, addrs))
}
