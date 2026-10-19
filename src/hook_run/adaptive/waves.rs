//! The recall waves of one prompt: the project recall, the all-projects recall, and the fanouts.

use std::time::Instant;

use super::{FANOUT_LIMIT, MAIN_LIMIT, Outcome, WAVE2_CUTOFF, query, recall};
use crate::hook_run::action::{RecallQuery, split_recall_records};
use crate::hook_run::mcp_client::McpHttpClient;
use crate::hook_run::relevance;

/// The recall texts for `text`, in budget order: the session project's main
/// recall, the all-projects main recall, keyword fanout, topic fanout. The flag
/// says whether a main recall succeeded.
pub(super) async fn waves(
    client: &McpHttpClient,
    text: &str,
    keywords: &[String],
    project: Option<&str>,
) -> (Outcome, bool) {
    let scoped = project.map(|p| RecallQuery {
        project: Some(p.to_string()),
        ..query(text, MAIN_LIMIT)
    });
    let keyword = |i: usize| {
        keywords.get(i).map(|k| RecallQuery {
            keyword: Some(k.clone()),
            ..query(text, FANOUT_LIMIT)
        })
    };
    let start = Instant::now();
    let (own, main, kw0, kw1) = tokio::join!(
        recall(client, scoped),
        recall(client, Some(query(text, MAIN_LIMIT))),
        recall(client, keyword(0)),
        recall(client, keyword(1)),
    );
    let main_ok = matches!(own, Some(Ok(_))) || matches!(main, Some(Ok(_)));
    let mut outcome = Outcome::default();
    [own, main, kw0, kw1]
        .into_iter()
        .for_each(|r| outcome.push(r));
    if start.elapsed() < WAVE2_CUTOFF {
        let topics: Vec<String> = outcome
            .texts
            .iter()
            .flat_map(|t| split_recall_records(t))
            .filter_map(|r| relevance::record_topic(&r).map(str::to_string))
            .collect();
        let siblings = relevance::sibling_topics(&topics);
        let topic = |i: usize| {
            siblings.get(i).map(|t| RecallQuery {
                topic: Some(t.clone()),
                ..query(text, FANOUT_LIMIT)
            })
        };
        let (t0, t1) = tokio::join!(recall(client, topic(0)), recall(client, topic(1)));
        outcome.push(t0);
        outcome.push(t1);
    }
    (outcome, main_ok)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use wiremock::matchers::{body_string_contains, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn mount(server: &MockServer, needle: &str, body: &str) {
        let reply = json!({"jsonrpc": "2.0", "id": 1,
                           "result": {"content": [{"type": "text", "text": body}]}});
        Mock::given(method("POST"))
            .and(body_string_contains(needle))
            .respond_with(ResponseTemplate::new(200).set_body_json(reply))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_page_fetched_in_another_project_is_not_recalled() {
        let server = MockServer::start().await;
        mount(&server, "initialize", "").await;
        mount(
            &server,
            r#""project":"proj-b""#,
            "[web-fetch-proj-b] URL: https://own.example",
        )
        .await;
        mount(
            &server,
            r#""project":"""#,
            "[web-fetch-proj-a] URL: https://other.example\n[web-fetch] URL: \
             https://old.example\n[context-proj-a] a fact",
        )
        .await;
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        let (outcome, main_ok) = waves(&client, "q", &[], Some("proj-b")).await;
        assert!(main_ok);
        let all = outcome.texts.join("\n");
        assert!(all.contains("https://own.example"), "{all}");
        assert!(all.contains("a fact"), "{all}");
        assert!(!all.contains("https://other.example"), "{all}");
        assert!(!all.contains("https://old.example"), "{all}");
    }
}
