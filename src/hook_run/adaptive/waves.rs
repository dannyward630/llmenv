//! The recall waves of one prompt: the project recall, the global topic recall, and the fanouts.

use std::time::Instant;

use super::{AdaptiveCtx, FANOUT_LIMIT, MAIN_LIMIT, Outcome, WAVE2_CUTOFF, query, recall};
use crate::hook_run::action::{RecallQuery, split_recall_records};
use crate::hook_run::relevance;

/// The recall texts for `text`, in budget order: the session project's main
/// recall, the recall of the global topics, keyword fanout, topic fanout. The flag
/// says whether a main recall succeeded. No main recall reads every project, so the
/// records of another project stay out (#2950).
pub(super) async fn waves(
    ctx: &AdaptiveCtx<'_>,
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
    let client = ctx.client;
    let start = Instant::now();
    let (own, main, kw0, kw1) = tokio::join!(
        recall(client, scoped),
        crate::hook_run::global_topics::recall(client, text, ctx.global_topics, MAIN_LIMIT),
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
    use crate::hook_run::mcp_client::McpHttpClient;

    async fn run(server: &MockServer, project: Option<&str>, topics: &[String]) -> (Outcome, bool) {
        let client = McpHttpClient::test_new(server.uri(), Duration::from_secs(2)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = crate::hook_run::session_ledger::LedgerStore::new(dir.path());
        let payload = json!({});
        let ctx = AdaptiveCtx {
            client: &client,
            store: &store,
            session_id: "s1",
            payload: &payload,
            global_topics: topics,
        };
        waves(&ctx, "q", &[], project).await
    }

    /// Matches a request whose body names no topic.
    struct NoTopic;

    impl wiremock::Match for NoTopic {
        fn matches(&self, request: &wiremock::Request) -> bool {
            !String::from_utf8_lossy(&request.body).contains(r#""topic""#)
        }
    }

    fn topics(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

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
        let (outcome, main_ok) = run(&server, Some("proj-b"), &topics(&["preferences"])).await;
        assert!(main_ok);
        let all = outcome.texts.join("\n");
        assert!(all.contains("https://own.example"), "{all}");
        assert!(all.contains("a fact"), "{all}");
        assert!(!all.contains("https://other.example"), "{all}");
        assert!(!all.contains("https://old.example"), "{all}");
    }

    #[tokio::test]
    async fn a_scope_record_of_another_project_is_not_recalled() {
        let server = MockServer::start().await;
        mount(&server, "initialize", "").await;
        mount(
            &server,
            r#""project":"""#,
            "[llmenv-scope-context] tags: lang-go, bundles: go-dev\n[context-proj-a] a fact",
        )
        .await;
        let (outcome, main_ok) = run(&server, None, &topics(&["preferences"])).await;
        assert!(main_ok);
        let all = outcome.texts.join("\n");
        assert!(all.contains("a fact"), "{all}");
        assert!(!all.contains("lang-go"), "{all}");
    }

    /// The main recall reads the session's project and the global topics only, so a record of
    /// another project in a topic that is not global stays out (#2950).
    #[tokio::test]
    async fn only_the_global_topics_are_read_across_projects() {
        let server = MockServer::start().await;
        mount(&server, "initialize", "").await;
        let reply = json!({"jsonrpc": "2.0", "id": 1,
                           "result": {"content": [{"type": "text", "text": "[preferences] be terse"}]}});
        Mock::given(method("POST"))
            .and(body_string_contains(r#""topic":"preferences""#))
            .respond_with(ResponseTemplate::new(200).set_body_json(reply))
            .with_priority(1)
            .mount(&server)
            .await;
        // An untopiced recall across every project is the leak, so only it answers with a record
        // of another project. A topic recall reads one topic, which the mock cannot filter.
        let reply = json!({"jsonrpc": "2.0", "id": 1,
                           "result": {"content": [{"type": "text", "text": "[context-proj-a] other project fact"}]}});
        Mock::given(method("POST"))
            .and(body_string_contains(r#""project":"""#))
            .and(NoTopic)
            .respond_with(ResponseTemplate::new(200).set_body_json(reply))
            .mount(&server)
            .await;
        mount(
            &server,
            r#""project":"proj-b""#,
            "[context-proj-b] own fact",
        )
        .await;

        let (outcome, main_ok) = run(&server, Some("proj-b"), &topics(&["preferences", " "])).await;
        assert!(main_ok);
        let all = outcome.texts.join("\n");
        assert!(
            all.contains("own fact") && all.contains("be terse"),
            "{all}"
        );
        assert!(!all.contains("other project fact"), "{all}");

        let (outcome, _) = run(&server, Some("proj-b"), &[]).await;
        let all = outcome.texts.join("\n");
        assert!(
            all.contains("own fact") && !all.contains("other project fact"),
            "{all}"
        );
    }
}
