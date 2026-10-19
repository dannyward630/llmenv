//! Decode the JSON-RPC replies of an MCP server: plain JSON or an SSE stream, the initialize
//! info, a `tools/list` page, and the text of a tool result.

use anyhow::{Context as _, anyhow};
use serde_json::Value;

use super::{InitializeInfo, ToolSummary};

/// Decode a JSON-RPC reply body, which is plain JSON or an SSE stream. A plain body is returned
/// as it is. An SSE stream gives the first `data:` event that has a `result` or an `error`, so a
/// notification event ahead of the reply is skipped.
pub(super) fn parse_rpc_body(body: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(body.trim()) {
        return Some(value);
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .find(|v| v.get("result").is_some() || v.get("error").is_some())
}

/// Read `result.instructions` from an `initialize` reply.
pub(super) fn parse_initialize_info(body: &str) -> anyhow::Result<InitializeInfo> {
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
pub(super) fn extract_text(body: &Value) -> Option<String> {
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
