//! The `type` value that every engine writes for a remote MCP server.

use crate::config::McpTransport;

/// Map a resolved remote transport onto the `type` discriminator string shared
/// by every engine's remote-MCP config shape (`"http"` / `"sse"`).
///
/// A stdio server never reaches this function. `crate::mcp::resolve` resolves a stdio server to
/// `ResolvedKind::Stdio`, so a stdio value here is a resolver bug. The function panics rather
/// than write a wrong `type` into a config (#2992).
pub(crate) fn remote_transport_type_str(transport: McpTransport) -> &'static str {
    match transport {
        McpTransport::Sse => "sse",
        McpTransport::Http => "http",
        McpTransport::Stdio => {
            unreachable!("a stdio MCP server resolves to ResolvedKind::Stdio, not a remote one")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_http_and_sse() {
        assert_eq!(remote_transport_type_str(McpTransport::Http), "http");
        assert_eq!(remote_transport_type_str(McpTransport::Sse), "sse");
    }

    #[test]
    #[should_panic(expected = "stdio MCP server")]
    fn stdio_panics_instead_of_writing_a_wrong_type() {
        let _ = remote_transport_type_str(McpTransport::Stdio);
    }
}
