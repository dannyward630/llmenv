//! Bounded reads of an MCP reply body. The body is read in chunks up to a cap, so a server
//! that sends a huge or endless reply cannot use up the memory of a hook.

use anyhow::Context as _;

/// The largest reply body llmenv reads. A recall reply or a wake-up pack is a few KB, so 8 MiB
/// leaves wide room and still bounds a reply from a faulty server.
pub(super) const MAX_REPLY_BYTES: usize = 8 * 1024 * 1024;
/// The part of an error body kept for the error message.
const MAX_ERROR_BODY_BYTES: usize = 4 * 1024;

/// The body of `resp` as text, read up to `max` bytes. Bytes that are not UTF-8 are replaced.
///
/// # Errors
/// The read fails, or the body holds more than `max` bytes.
pub(super) async fn read_capped(
    mut resp: reqwest::Response,
    max: usize,
    label: &str,
) -> anyhow::Result<String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)
        .with_context(|| format!("reading the response body for {label}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max {
            anyhow::bail!(
                "the reply for {label} holds more than {max} bytes, so llmenv did not read it. \
                 Fix: check the MCP server, which sent an oversized reply"
            );
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The start of an error body for an error message, at most [`MAX_ERROR_BODY_BYTES`] bytes.
/// A read failure is logged, and the text says that the body could not be read.
pub(super) async fn error_body(mut resp: reqwest::Response, label: &str) -> String {
    let mut bytes = Vec::new();
    let mut cut = false;
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                let room = MAX_ERROR_BODY_BYTES.saturating_sub(bytes.len());
                bytes.extend(chunk.iter().take(room));
                if chunk.len() > room {
                    cut = true;
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => {
                tracing::error!(error = %e, request = %label, "failed to read MCP error response body");
                if bytes.is_empty() {
                    return "(failed to read error body)".to_string();
                }
                break;
            }
        }
    }
    let text = String::from_utf8_lossy(&bytes);
    if cut {
        format!("{text} [cut at {MAX_ERROR_BODY_BYTES} bytes]")
    } else {
        text.into_owned()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn reply(body: Vec<u8>) -> reqwest::Response {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;
        reqwest::get(server.uri()).await.unwrap()
    }

    #[tokio::test]
    async fn a_reply_within_the_cap_is_read_whole() {
        let text = read_capped(reply(b"hello".to_vec()).await, 5, "t")
            .await
            .unwrap();
        assert_eq!(text, "hello");
    }

    #[tokio::test]
    async fn a_reply_over_the_cap_is_an_error_that_names_the_limit() {
        let err = read_capped(reply(vec![b'x'; 6]).await, 5, "tool t")
            .await
            .unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text.contains("more than 5 bytes") && text.contains("tool t"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn a_long_error_body_is_cut() {
        let body = error_body(reply(vec![b'e'; MAX_ERROR_BODY_BYTES + 10]).await, "t").await;
        assert!(
            body.ends_with(&format!("[cut at {MAX_ERROR_BODY_BYTES} bytes]")),
            "{body}"
        );
        let kept = body.split(" [cut at").next().unwrap();
        assert_eq!(kept.len(), MAX_ERROR_BODY_BYTES);
        assert!(kept.bytes().all(|b| b == b'e'));
        assert_eq!(
            error_body(reply(b"short".to_vec()).await, "t").await,
            "short"
        );
    }

    /// The caps are the sizes that the docs and the changelog state: 8 MiB and 4 KiB.
    #[tokio::test]
    async fn the_reply_cap_is_8_mib() {
        let at_cap = read_capped(reply(vec![b'x'; 8_388_608]).await, MAX_REPLY_BYTES, "t").await;
        assert_eq!(at_cap.unwrap().len(), 8_388_608);
        let over = read_capped(reply(vec![b'x'; 8_388_609]).await, MAX_REPLY_BYTES, "t").await;
        assert!(over.is_err());
    }

    #[tokio::test]
    async fn an_error_body_that_exactly_fills_the_cap_is_not_cut() {
        let body = error_body(reply(vec![b'e'; 4096]).await, "t").await;
        assert_eq!(body.len(), 4096, "no cut marker: {}", &body[4090..]);
        let body = error_body(reply(vec![b'e'; 4097]).await, "t").await;
        assert!(body.ends_with("[cut at 4096 bytes]"), "{}", &body[4090..]);
    }
}
