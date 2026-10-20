use proptest::prelude::*;

/// A small recursive JSON generator: scalars, then arrays/objects of
/// them. Was duplicated byte-for-byte in three places (`llmenv-util`,
/// `src/adapter/mod.rs`, `src/adapter/claude_code.rs`) before #1281.
pub fn arb_json() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i32>().prop_map(serde_json::Value::from),
        "[a-z]{0,4}".prop_map(serde_json::Value::String),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(serde_json::Value::Array),
            prop::collection::vec(("[a-z]{1,4}", inner), 0..4)
                .prop_map(|kvs| serde_json::Value::Object(kvs.into_iter().collect())),
        ]
    })
}

// -- Hook-field string generators (#1281) --
//
// `llmenv`'s `crate::config::Hook` and `llmenv-config`'s `schema::Hook`
// are two distinct, non-interchangeable types (one per crate) — this
// module can't generate either one directly, and forcing them into a
// single shared type isn't worth it for a test-only generator. What
// *was* a genuine, safe-to-share duplicate is the escaping-relevant
// character classes each field's strategy uses below: added to
// `llmenv`'s `src/adapter/skills.rs::arb_hook_handler` during #1265's
// pre-pr-review (a plain-alnum-only charset couldn't stress the JSON
// escaping the `opencode`/`claude_code` renderers rely on when splicing
// hook strings into their output), but never carried over to
// `llmenv-config`'s `validate.rs::arb_hook`, which kept a plain alnum
// charset for the same three fields. Sharing the charset here — instead
// of each crate's `arb_hook`-equivalent inlining its own regex literal —
// is what actually closes the "more permissive character set" gap #1281
// found, without needing the two `Hook` types to be the same type.
//
// `arb_yaml_value` was considered for the same treatment and rejected:
// `llmenv-config::validate::arb_yaml_value` and `llmenv::skills::
// arb_yaml_value` generate meaningfully different shapes for different
// purposes (the former covers YAML-ambiguous scalars, tagged values, and
// non-string mapping keys for round-trip fidelity; the latter is a
// simpler generator for native-fragment merge fuzzing) — unifying them
// would either weaken the round-trip coverage or bloat the merge-fuzz
// generator with cases irrelevant to what it tests.

/// A hook `matcher` string — an event glob/regex fragment. Plain
/// `String`, not `Option<String>` (a hook commonly has no matcher) —
/// callers wrap in `proptest::option::of` themselves, matching
/// [`arb_hook_command_str`]/[`arb_hook_tool_str`]'s interface.
pub fn arb_hook_matcher() -> impl Strategy<Value = String> {
    r#"[a-zA-Z0-9*][a-zA-Z0-9*\\"'$`{}\n]{0,7}"#
}

/// A hook's `command` string (the `Command`-kind handler's shell
/// command). Charset covers path-like shapes (`.`, `/`, `-`) and digits
/// alongside the escaping-relevant characters — digits so this doesn't
/// regress `llmenv-config`'s prior plain-alnum coverage (#1281
/// pre-pr-review finding: dropping digits loses the YAML
/// scalar-ambiguity class of round-trip bug, e.g. an unquoted `123`).
pub fn arb_hook_command_str() -> impl Strategy<Value = String> {
    r#"[a-z0-9][a-z0-9 ./\\"'$`{}\n-]{0,20}"#
}

/// A hook's `tool` string (the `McpTool`-kind handler's MCP tool name).
/// Charset covers underscore-separated identifier shapes and digits
/// alongside the escaping-relevant characters (see
/// [`arb_hook_command_str`] for why digits matter here).
pub fn arb_hook_tool_str() -> impl Strategy<Value = String> {
    r#"[a-z0-9_][a-z0-9_\\"'$`{}\n]{0,15}"#
}

/// Fixed advisory-lock address serializing every test, in any workspace
/// crate, that allocates an ephemeral TCP port (#1481, extended to the
/// main `llmenv` crate in #1610). `cargo test`/`cargo nextest` can run
/// each crate's test binaries as separate, concurrent processes — `cargo
/// nextest` runs every individual test as its own process — so an
/// in-process `Mutex` cannot stop one binary's test from reusing a port
/// another just freed. Binding a fixed, otherwise unused port as an
/// advisory lock works across the process boundary: only one bind can
/// hold it at a time, anywhere in the workspace.
///
/// Deliberately below both platforms' default ephemeral ranges (Linux
/// `net.ipv4.ip_local_port_range` 32768-60999, macOS 49152-65535) — a
/// port inside either range can be handed to an unrelated process by the
/// kernel, which would make this lock spin out its retry budget and
/// panic on a squatter that has nothing to do with test contention
/// (#1497).
const NETWORK_TEST_LOCK_ADDR: &str = "127.0.0.1:20990";

/// Whether a bind failure is contention with a sibling test binary
/// holding the lock (worth retrying) versus a different failure —
/// permission denied, address unavailable, fd exhaustion — that won't
/// resolve itself by waiting (#1497).
fn is_retryable_bind_error(kind: std::io::ErrorKind) -> bool {
    kind == std::io::ErrorKind::AddrInUse
}

/// Acquires [`NETWORK_TEST_LOCK_ADDR`], retrying until it succeeds.
///
/// Only retries on genuine contention (see [`is_retryable_bind_error`]);
/// any other bind error panics immediately with the real `io::Error`
/// rather than burning the full retry budget and then blaming "another
/// process" for a cause that was never contention (#1497).
///
/// # Panics
/// Panics if the lock cannot be acquired within the retry budget, or on
/// a non-contention bind error.
#[must_use]
#[expect(
    clippy::panic,
    reason = "test scaffolding — a stuck advisory lock is a hard test failure"
)]
pub fn port_guard() -> std::net::TcpListener {
    let mut last_err = None;
    for _ in 0..250 {
        match std::net::TcpListener::bind(NETWORK_TEST_LOCK_ADDR) {
            Ok(l) => return l,
            Err(e) if is_retryable_bind_error(e.kind()) => last_err = Some(e),
            Err(e) => panic!(
                "unexpected error binding the network-test advisory lock on \
                 {NETWORK_TEST_LOCK_ADDR}: {e} (kind: {:?}) — this is not address \
                 contention, retrying would not help",
                e.kind()
            ),
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!(
        "could not acquire the network-test advisory lock on {NETWORK_TEST_LOCK_ADDR} after \
         5s (last error: {}); is another process bound to it?",
        last_err.map_or_else(|| "unknown".to_string(), |e| e.to_string())
    );
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test scaffolding")]
mod port_guard_tests {
    use super::{NETWORK_TEST_LOCK_ADDR, is_retryable_bind_error};

    #[test]
    fn network_test_lock_addr_is_outside_default_ephemeral_ranges() {
        let port: u16 = NETWORK_TEST_LOCK_ADDR
            .rsplit(':')
            .next()
            .expect("address has a port")
            .parse()
            .expect("port is numeric");
        assert!(
            port < 32768,
            "port {port} falls inside Linux's default ephemeral range (32768-60999)"
        );
        assert!(
            port < 49152,
            "port {port} falls inside macOS's default ephemeral range (49152-65535)"
        );
    }

    #[test]
    fn only_retries_on_addr_in_use() {
        assert!(is_retryable_bind_error(std::io::ErrorKind::AddrInUse));
        assert!(!is_retryable_bind_error(
            std::io::ErrorKind::PermissionDenied
        ));
    }
}
