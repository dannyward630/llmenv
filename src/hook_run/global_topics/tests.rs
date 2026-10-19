use super::*;

fn topics(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_string()).collect()
}

#[test]
fn each_named_topic_is_one_recall_across_projects() {
    let queries = topic_queries("q", &topics(&["preferences", " ", "errors-resolved "]), 4);
    let named: Vec<_> = queries
        .iter()
        .map(|q| q.topic.as_deref().unwrap())
        .collect();
    assert_eq!(named, ["preferences", "errors-resolved"]);
    assert!(queries.iter().all(|q| q.project.as_deref() == Some("")));
    assert!(queries.iter().all(|q| q.limit == 4 && q.query == "q"));
}

#[test]
fn the_stateless_turn_recall_never_reads_every_project() {
    let global = topics(&["preferences"]);
    let unscoped = turn_actions(vec![Action::Recall(String::new())], "q", &global);
    assert!(
        unscoped.iter().all(|a| !matches!(a, Action::Recall(_))),
        "the empty project recall reads every project: {unscoped:?}"
    );
    assert_eq!(unscoped.len(), 1, "{unscoped:?}");

    let scoped = turn_actions(vec![Action::Recall("proj".into())], "q", &global);
    assert!(matches!(&scoped[0], Action::Recall(p) if p == "proj"));
    assert_eq!(
        scoped.len(),
        2,
        "the project and the global topic: {scoped:?}"
    );

    assert!(
        turn_actions(vec![Action::Store], "q", &global).len() == 1,
        "no recall, no topics"
    );
    assert!(turn_actions(vec![Action::Recall(String::new())], "q", &[]).is_empty());
}
