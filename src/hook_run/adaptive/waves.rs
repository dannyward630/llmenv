//! The recall waves of one prompt: the project recall, the global topic recall, and the fanouts.

use std::time::Instant;

use super::{AdaptiveCtx, FANOUT_LIMIT, MAIN_LIMIT, Outcome, WAVE2_CUTOFF, query, recall};
use crate::hook_run::action::{RecallQuery, split_recall_records};
use crate::hook_run::relevance;

/// The recall texts for `text`, in budget order: the session project's main
/// recall, the recall of the global topics, keyword fanout, topic fanout. The flag
/// says whether the main recalls ran and none of them failed. Only the global topics are
/// read across projects, so the records of another project stay out (#2950). With no
/// session project, the fanouts do not run.
pub(super) async fn waves(
    ctx: &AdaptiveCtx<'_>,
    text: &str,
    keywords: &[String],
    project: Option<&str>,
) -> (Outcome, bool) {
    let in_project = |q: RecallQuery| {
        project.map(|p| RecallQuery {
            project: Some(p.to_string()),
            ..q
        })
    };
    let scoped = in_project(query(text, MAIN_LIMIT));
    let keyword = |i: usize| {
        keywords.get(i).and_then(|k| {
            in_project(RecallQuery {
                keyword: Some(k.clone()),
                ..query(text, FANOUT_LIMIT)
            })
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
    // A failed project recall is not a finished query, even when a global topic answered: the
    // skip rule would reuse it and the project's records would stay missing.
    let main_ok = (own.is_some() || main.is_some())
        && !matches!(own, Some(Err(_)))
        && !matches!(main, Some(Err(_)));
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
            siblings.get(i).and_then(|t| {
                in_project(RecallQuery {
                    topic: Some(t.clone()),
                    ..query(text, FANOUT_LIMIT)
                })
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
        run_with_keywords(server, project, topics, &[]).await
    }

    async fn run_with_keywords(
        server: &MockServer,
        project: Option<&str>,
        topics: &[String],
        keywords: &[String],
    ) -> (Outcome, bool) {
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
        waves(&ctx, "q", keywords, project).await
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

    /// The keyword and sibling-topic fanouts read the session project only, so no request reads
    /// every project when no global topic is set (#2950).
    #[tokio::test]
    async fn the_fanouts_read_only_the_session_project() {
        let server = MockServer::start().await;
        mount(&server, "initialize", "").await;
        mount(
            &server,
            r#""project":"""#,
            "[context-proj-a] other project fact",
        )
        .await;
        mount(
            &server,
            r#""project":"proj-b""#,
            "[context-proj-b] own fact",
        )
        .await;

        let keywords = topics(&["k1", "k2"]);
        let (outcome, main_ok) = run_with_keywords(&server, Some("proj-b"), &[], &keywords).await;
        assert!(main_ok);
        let all = outcome.texts.join("\n");
        assert!(all.contains("own fact"), "{all}");
        assert!(!all.contains("other project fact"), "{all}");
        // The main recall, two keyword fanouts, and two sibling-topic fanouts.
        assert_eq!(outcome.calls, 5, "{outcome:?}");
    }

    /// With no session project, only the global topics are read.
    #[tokio::test]
    async fn no_session_project_runs_no_fanout() {
        let server = MockServer::start().await;
        mount(&server, "initialize", "").await;
        mount(
            &server,
            r#""topic":"preferences""#,
            "[preferences] be terse",
        )
        .await;

        let keywords = topics(&["k1"]);
        let (outcome, main_ok) =
            run_with_keywords(&server, None, &topics(&["preferences"]), &keywords).await;
        assert!(main_ok);
        assert_eq!(outcome.calls, 1, "{outcome:?}");
        assert!(outcome.texts.join("\n").contains("be terse"));
    }

    /// A failed project recall is not a finished query, even when a global topic answered.
    #[tokio::test]
    async fn a_failed_project_recall_is_not_ok_beside_a_global_topic() {
        let server = MockServer::start().await;
        mount(&server, "initialize", "").await;
        mount(
            &server,
            r#""topic":"preferences""#,
            "[preferences] be terse",
        )
        .await;
        Mock::given(method("POST"))
            .and(body_string_contains(r#""project":"proj-b""#))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let (outcome, main_ok) = run(&server, Some("proj-b"), &topics(&["preferences"])).await;
        assert!(!main_ok, "{outcome:?}");
        assert!(outcome.texts.join("\n").contains("be terse"));
    }
}
