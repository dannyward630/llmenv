//! Minimal HTTP JSON-RPC MCP client — only the `tools/call` path this feature
//! needs. Not a general MCP library.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, anyhow};
use serde_json::{Value, json};
use url::{Host, Url};

use crate::mcp::cleartext::{embedded_ipv4, is_cleartext_safe, is_unique_local_v6};

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
pub(crate) enum SsrfPolicy {
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

/// A minimal MCP-over-HTTP client bound to one server URL with a fixed timeout.
#[derive(Debug, Clone)]
pub struct McpHttpClient {
    url: String,
    client: reqwest::Client,
    /// MCP's Streamable HTTP transport is session-scoped: the server hands out
    /// an `Mcp-Session-Id` on `initialize` and rejects `tools/call` without it
    /// ("Bad Request: Missing session ID"). Negotiated lazily on first call and
    /// cached so repeat calls on the same client reuse it. `tokio::sync::Mutex`
    /// (not `std::sync::Mutex`) because it's held across the `.await` in
    /// `ensure_session` — the workspace denies `await_holding_lock` for the
    /// std variant.
    session_id: std::sync::Arc<tokio::sync::Mutex<Option<String>>>,
    /// What the server said in its last `initialize` reply (#2148).
    init_info: std::sync::Arc<tokio::sync::Mutex<Option<InitializeInfo>>>,
}

/// What an MCP server reports about itself in its `initialize` reply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InitializeInfo {
    pub instructions: Option<String>,
}

/// One tool of a server's `tools/list` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolSummary {
    pub name: String,
    pub description: Option<String>,
}

/// The most `tools/list` pages [`McpHttpClient::list_tools`] follows, so a server that always
/// returns a cursor cannot loop doctor.
pub(crate) const MAX_TOOL_PAGES: usize = 50;

/// Internal result of one [`McpHttpClient::try_call_tool`] attempt: whether a
/// failure is worth clearing the cached session and retrying once (#1094), or
/// is fatal on the first try.
enum CallToolError {
    /// The server rejected the cached `Mcp-Session-Id` (HTTP 400/404) —
    /// clearing it and re-initializing may still succeed.
    StaleSession(anyhow::Error),
    /// Any other failure — retrying with a fresh session wouldn't help.
    Fatal(anyhow::Error),
}

impl From<CallToolError> for anyhow::Error {
    fn from(e: CallToolError) -> Self {
        match e {
            CallToolError::StaleSession(e) | CallToolError::Fatal(e) => e,
        }
    }
}

/// Whether `status` is the MCP Streamable HTTP transport's shape for "this
/// session id isn't valid" (mcp-proxy itself uses 400 for "Bad Request:
/// Missing session ID"; a dead/expired session is 404). Over-broad by
/// design: a 400 for an unrelated reason (e.g. malformed tool arguments)
/// also takes the stale-session retry path, wasting one re-initialize
/// before the real error surfaces on the second attempt — but it always
/// does surface, just one round trip later.
fn is_stale_session_status(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::NOT_FOUND
    )
}

/// Emit `[LLMENV_MCP_CALL] <tool> <duration>us` to stderr when
/// `LLMENV_TRACE_TIMING` is set (#1259) — the same env var that already gates
/// hook-run's per-phase timing markers and the cache hit/miss telemetry, so
/// there's one flag to know about, not three. Bare `eprintln!`, matching
/// `emit_trace_timing`'s precedent, for the same reason: a machine-parseable
/// protocol line must not depend on the tracing subscriber's config.
///
/// Deliberately reports timing only, not a result/entry count (the issue's
/// own examples show a count suffix only for `icm_memory_recall`, none for
/// `icm_memory_store`) — this client is a generic MCP transport with no
/// notion of what a "result" means for any given tool, and parsing ICM's
/// recall-specific text format here would be a layering violation.
fn emit_mcp_call_trace(tool: &str, elapsed: Duration) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    let us = elapsed.as_micros();
    eprintln!("[LLMENV_MCP_CALL] {tool} {us}us");
}

impl McpHttpClient {
    /// Build a client for `url` whose every request is bounded by `timeout`.
    ///
    /// # Errors
    /// Returns an error if the URL is invalid, uses an unsupported scheme, or
    /// points to a private/loopback IP address (SSRF protection).
    pub fn new(url: String, timeout: Duration) -> anyhow::Result<Self> {
        Self::with_headers(url, timeout, &std::collections::BTreeMap::new())
    }

    /// [`Self::new`] that also sends `headers` on every request, so a `Remote` entry's
    /// configured headers reach the probe (#2148).
    ///
    /// # Errors
    /// As [`Self::new`], and when a header name or value is not valid HTTP.
    pub(crate) fn with_headers(
        url: String,
        timeout: Duration,
        headers: &std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<Self> {
        // Resolve and SSRF-validate up front, then pin reqwest to exactly the vetted
        // addresses. Pinning closes the DNS-rebinding TOCTOU: reqwest never re-resolves at
        // send() time, so the connection can only target an address we already approved.
        let (host, addrs) =
            validate_url_production(&url, SsrfPolicy::AllowPrivateNetwork, timeout)?;
        require_https_for_public(&url, &addrs)?;
        let mut map = reqwest::header::HeaderMap::new();
        for (name, value) in headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("MCP header name '{name}' is not valid"))?;
            let mut value = reqwest::header::HeaderValue::from_str(value)
                .with_context(|| format!("MCP header '{name}' has a value that is not valid"))?;
            value.set_sensitive(true);
            map.insert(name, value);
        }
        // A redirect could reach a host that the address pin above does not cover, such as a
        // cloud metadata address, and could carry configured headers with it. An MCP endpoint
        // has no reason to redirect, so no redirect is followed.
        // A proxy resolves the host itself, so it would bypass the address pin above (#2484).
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .default_headers(map)
            .no_proxy()
            .resolve_to_addrs(&host, &addrs)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build HTTP client (TLS backend unavailable)")?;
        Ok(Self {
            url,
            client,
            session_id: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            init_info: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    #[cfg(test)]
    /// Build a client for testing, skipping SSRF validation.
    pub(crate) fn test_new(url: String, timeout: Duration) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            url,
            client,
            session_id: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            init_info: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    /// The server URL without credentials or a query string, for error text that reaches a
    /// terminal.
    fn display_url(&self) -> String {
        redact_url(&self.url)
    }

    /// Negotiate an MCP session if one hasn't already been established on this
    /// client: send `initialize`, capture the `Mcp-Session-Id` response header
    /// (if the server sends one — plain non-session-scoped servers won't), and
    /// best-effort acknowledge with `notifications/initialized`.
    ///
    /// # Errors
    /// Network failure or a non-2xx status on the `initialize` request itself.
    async fn ensure_session(&self) -> anyhow::Result<Option<String>> {
        self.ensure_session_with(false).await
    }

    /// [`Self::ensure_session`]. With `capture_info`, the `initialize` reply body is read into
    /// `init_info`; the other callers never read it.
    async fn ensure_session_with(&self, capture_info: bool) -> anyhow::Result<Option<String>> {
        let mut cached = self.session_id.lock().await;
        if let Some(sid) = cached.as_ref() {
            return Ok(Some(sid.clone()));
        }

        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "llmenv", "version": env!("CARGO_PKG_VERSION") }
            }
        });
        let resp = self
            .client
            .post(&self.url)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .json(&init_req)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("POST {} for MCP initialize", self.display_url()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .inspect_err(|e| {
                    tracing::warn!(
                        error = %e,
                        url = %self.url,
                        "failed to read MCP initialize error response body"
                    )
                })
                .unwrap_or_else(|_| "(failed to read error body)".to_string());
            return Err(anyhow!("MCP initialize returned HTTP {status}: {body}"));
        }
        let sid = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if capture_info {
            let text = resp
                .text()
                .await
                .map_err(reqwest::Error::without_url)
                .with_context(|| {
                    format!(
                        "reading the MCP initialize reply from {}",
                        self.display_url()
                    )
                })?;
            *self.init_info.lock().await = Some(parse_initialize_info(&text)?);
        }

        if let Some(sid) = &sid {
            // Best-effort: some servers require this ack before accepting
            // further calls, others ignore it. Its own failure isn't fatal —
            // the subsequent tools/call attempt is the real signal.
            let _ = self
                .client
                .post(&self.url)
                .header(
                    reqwest::header::ACCEPT,
                    "application/json, text/event-stream",
                )
                .header("mcp-session-id", sid)
                .json(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .send()
                .await;
        }

        *cached = sid.clone();
        Ok(sid)
    }

    /// Check that the server answers a fresh MCP `initialize` within the client timeout.
    ///
    /// # Errors
    /// Network failure, timeout, or a non-2xx status.
    pub async fn probe(&self) -> anyhow::Result<()> {
        // Drop any cached session: a probe must ask the server, not trust an old id.
        *self.session_id.lock().await = None;
        self.ensure_session().await.map(|_| ())
    }

    /// Ask the server for a fresh `initialize` and return what it said about itself.
    ///
    /// # Errors
    /// Network failure, timeout, a non-2xx status, or a reply with no readable `result`.
    pub(crate) async fn initialize_info(&self) -> anyhow::Result<InitializeInfo> {
        // Drop any cached session: the info must come from a fresh `initialize`.
        *self.session_id.lock().await = None;
        self.ensure_session_with(true).await?;
        Ok(self.init_info.lock().await.clone().unwrap_or_default())
    }

    /// List every tool of the server, following `nextCursor` for at most [`MAX_TOOL_PAGES`] pages.
    /// A session the server no longer knows is re-initialized once, as in [`Self::call_tool`].
    ///
    /// # Errors
    /// Network failure, a non-2xx status, a JSON-RPC `error`, a reply with no `result.tools`
    /// array, or more than [`MAX_TOOL_PAGES`] pages.
    pub(crate) async fn list_tools(&self) -> anyhow::Result<Vec<ToolSummary>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_TOOL_PAGES {
            let params = cursor
                .as_ref()
                .map_or_else(|| json!({}), |c| json!({ "cursor": c }));
            let body = match self.post_rpc("tools/list", &params, "tools/list").await {
                Ok(body) => body,
                Err(CallToolError::StaleSession(first_err)) => {
                    *self.session_id.lock().await = None;
                    self.post_rpc("tools/list", &params, "tools/list")
                        .await
                        .map_err(anyhow::Error::from)
                        .with_context(|| {
                            format!("retry after stale session; first attempt: {first_err}")
                        })?
                }
                Err(CallToolError::Fatal(e)) => return Err(e),
            };
            let (page, next) = parse_tools_page(body.get("result").unwrap_or(&Value::Null))?;
            tools.extend(page);
            cursor = next;
            if cursor.is_none() {
                return Ok(tools);
            }
        }
        Err(anyhow!(
            "tools/list returned more than {MAX_TOOL_PAGES} pages; stopped at the cap"
        ))
    }

    /// Call one MCP tool and return the concatenated text content.
    ///
    /// A cached `Mcp-Session-Id` the server no longer recognizes (expired, or
    /// the server restarted) comes back as HTTP 400 or 404 per the MCP
    /// Streamable HTTP transport (#1094: mcp-proxy itself uses 400 for
    /// "Bad Request: Missing session ID"). Rather than replaying that dead id
    /// forever — the only recovery previously being a fresh `llmenv` process —
    /// this clears the cache and re-initializes exactly once before giving up.
    ///
    /// # Errors
    /// Network failure, session negotiation failure, timeout, non-2xx status
    /// (after the one stale-session retry), a JSON-RPC `error` field, or a
    /// response missing `result.content[].text`.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> anyhow::Result<String> {
        let start = std::time::Instant::now();
        let result = match self.try_call_tool(name, &arguments).await {
            Ok(text) => Ok(text),
            Err(CallToolError::StaleSession(first_err)) => {
                tracing::warn!(
                    error = %first_err,
                    tool = %name,
                    "cached MCP session rejected, re-initializing and retrying once"
                );
                *self.session_id.lock().await = None;
                self.try_call_tool(name, &arguments)
                    .await
                    .map_err(anyhow::Error::from)
                    .with_context(|| {
                        format!("retry after stale session; first attempt: {first_err}")
                    })
            }
            Err(CallToolError::Fatal(e)) => Err(e),
        };
        // Only the single entry point for every MCP tool call this process
        // makes (#1259) — includes a stale-session retry's full cost when one
        // happens, which is exactly the "MCP proxy latency" #1259 wants
        // visible. Emitted on success only, matching emit_trace_timing's and
        // emit_cache_trace's precedent (the failure itself already logs via
        // the tracing::warn!/Action::run's own error path).
        if result.is_ok() {
            emit_mcp_call_trace(name, start.elapsed());
        }
        result
    }

    /// One attempt at `call_tool`, distinguishing a stale-session rejection
    /// (worth clearing the cache and retrying once) from every other failure.
    async fn try_call_tool(&self, name: &str, arguments: &Value) -> Result<String, CallToolError> {
        let params = json!({ "name": name, "arguments": arguments });
        let body = self
            .post_rpc("tools/call", &params, &format!("tool {name}"))
            .await?;
        let text = extract_text(&body).ok_or_else(|| {
            CallToolError::Fatal(anyhow!(
                "tool {name} response missing result.content[].text"
            ))
        })?;
        // MCP reports a tool-level failure as a successful reply with `isError: true`. Treating
        // it as success would mark a failed store as stored in the idempotency set.
        if body["result"]["isError"].as_bool() == Some(true) {
            return Err(CallToolError::Fatal(anyhow!(
                "tool {name} reported an error: {text}"
            )));
        }
        Ok(text)
    }

    /// Send one JSON-RPC request on the session and return the decoded reply. `label` names the
    /// request in error text. The one request path for `tools/call` and `tools/list`.
    async fn post_rpc(
        &self,
        method: &str,
        params: &Value,
        label: &str,
    ) -> Result<Value, CallToolError> {
        let session_id = self.ensure_session().await.map_err(CallToolError::Fatal)?;

        let req = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let mut builder = self
            .client
            .post(&self.url)
            // MCP's Streamable HTTP transport requires the client to accept
            // both response shapes a server may reply with. Without this,
            // mcp-proxy (fronting icm serve) 406s every call, which the SSRF
            // loopback block used to mask entirely for same-host setups.
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            );
        if let Some(sid) = &session_id {
            builder = builder.header("mcp-session-id", sid);
        }
        let resp = builder
            .json(&req)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("POST {} for {label}", self.display_url()))
            .map_err(CallToolError::Fatal)?;

        // Capture status and body for detailed error reporting.
        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .inspect_err(|e| {
                    tracing::warn!(
                        error = %e,
                        request = %label,
                        "failed to read MCP error response body"
                    )
                })
                .unwrap_or_else(|_| "(failed to read error body)".to_string());
            let err = anyhow!("{label} returned HTTP {}: {}", status, body);
            return Err(if is_stale_session_status(status) {
                CallToolError::StaleSession(err)
            } else {
                CallToolError::Fatal(err)
            });
        }

        let text = resp
            .text()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("decoding JSON response for {label}"))
            .map_err(CallToolError::Fatal)?;
        let body = parse_rpc_body(&text)
            .ok_or_else(|| anyhow!("decoding JSON response for {label}: the reply is not JSON"))
            .map_err(CallToolError::Fatal)?;

        if let Some(err) = body.get("error") {
            return Err(CallToolError::Fatal(anyhow!(
                "{label} JSON-RPC error: {err}"
            )));
        }
        Ok(body)
    }
}

/// Decode a JSON-RPC reply body, which is plain JSON or an SSE stream. A plain body is returned
/// as it is. An SSE stream gives the first `data:` event that has a `result` or an `error`, so a
/// notification event ahead of the reply is skipped.
fn parse_rpc_body(body: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(body.trim()) {
        return Some(value);
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .find(|v| v.get("result").is_some() || v.get("error").is_some())
}

/// Read `result.instructions` from an `initialize` reply.
fn parse_initialize_info(body: &str) -> anyhow::Result<InitializeInfo> {
    let reply = parse_rpc_body(body).context("the initialize reply is not JSON or SSE")?;
    if let Some(error) = reply.get("error") {
        return Err(anyhow!("MCP initialize JSON-RPC error: {error}"));
    }
    let result = reply
        .get("result")
        .context("the initialize reply has no result")?;
    let instructions = match result.get("instructions") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => return Err(anyhow!("initialize instructions is not a string: {other}")),
    };
    Ok(InitializeInfo { instructions })
}

/// Read one `tools/list` `result`: its tools and its `nextCursor`. Both the HTTP and the stdio
/// probe use it, so the two cannot disagree.
///
/// # Errors
/// The result has no `tools` array, or a tool has no string `name` or a `description` that is not
/// a string.
pub(crate) fn parse_tools_page(
    result: &Value,
) -> anyhow::Result<(Vec<ToolSummary>, Option<String>)> {
    let listed = result
        .get("tools")
        .and_then(Value::as_array)
        .context("tools/list reply has no result.tools array")?;
    let mut tools = Vec::with_capacity(listed.len());
    for (index, tool) in listed.iter().enumerate() {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .with_context(|| format!("tools/list entry {index} has no string name"))?;
        let description = match tool.get("description") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(_) => {
                return Err(anyhow!(
                    "tool '{name}' has a description that is not a string"
                ));
            }
        };
        tools.push(ToolSummary {
            name: name.to_string(),
            description,
        });
    }
    let next = result
        .get("nextCursor")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(String::from);
    Ok((tools, next))
}

/// Pull and concatenate every `text` entry from `result.content[]`.
fn extract_text(body: &Value) -> Option<String> {
    let content = body.get("result")?.get("content")?.as_array()?;
    let mut out = String::new();
    for item in content {
        if let Some(t) = item.get("text").and_then(Value::as_str) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(t);
        }
    }
    Some(out)
}

/// Report why an IP address is blocked for SSRF safety, or `None` if it is a
/// routable public address.
///
/// Single source of truth shared by literal-IP and resolved-domain validation.
/// IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) are unwrapped and judged by
/// their IPv4 form so a blocked v4 range cannot be smuggled through the v6
/// namespace (#191).
fn blocked_reason(ip: &IpAddr, policy: SsrfPolicy) -> Option<&'static str> {
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
pub(crate) fn resolve_with_timeout(
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
fn run_with_timeout<T, F>(f: F, timeout: Duration) -> Result<T, mpsc::RecvTimeoutError>
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
fn redact_url(url: &str) -> String {
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
fn require_https_for_public(url: &str, addrs: &[SocketAddr]) -> anyhow::Result<()> {
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
fn drop_link_local_for_domain(
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
fn is_link_local(ip: &IpAddr) -> bool {
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
pub(crate) fn validate_url_production(
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
