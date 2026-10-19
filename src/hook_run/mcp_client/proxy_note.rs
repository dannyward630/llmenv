//! The note that a failed MCP request gets when a proxy variable is set (#2884). MCP requests
//! ignore proxies since #2484, so a user behind a required proxy otherwise sees only a timeout.

use std::ffi::OsString;

/// The variables that reqwest would read for a proxy, upper and lower case.
const PROXY_VARS: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// The docs section that explains why MCP requests skip the proxy.
const DOCS_URL: &str = "https://phaedrus1992.github.io/llmenv/docs/mcp#memory-backend-memory";

/// Add the proxy note to a failed request, when any proxy variable is set.
pub(super) fn with_proxy_note(error: anyhow::Error) -> anyhow::Error {
    match note(|name| std::env::var_os(name)) {
        Some(note) => error.context(note),
        None => error,
    }
}

/// The note that names each set proxy variable, or `None` when no variable is set. An empty
/// value counts as unset, as it does for reqwest.
fn note(get: impl Fn(&str) -> Option<OsString>) -> Option<String> {
    let set: Vec<&str> = PROXY_VARS
        .iter()
        .copied()
        .filter(|name| get(name).is_some_and(|value| !value.is_empty()))
        .collect();
    if set.is_empty() {
        return None;
    }
    Some(format!(
        "{} is set, but llmenv sends MCP requests without a proxy, so a server that this host \
         reaches only through the proxy fails here. Fix: make the MCP server reachable without \
         the proxy; see {DOCS_URL}",
        set.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use super::{PROXY_VARS, note};

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: BTreeMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn no_proxy_variable_gives_no_note() {
        assert_eq!(note(env(&[])), None);
        assert_eq!(
            note(env(&[("HTTPS_PROXY", "")])),
            None,
            "an empty value is unset"
        );
        assert_eq!(note(env(&[("NO_PROXY", "x")])), None);
    }

    #[test]
    fn the_note_names_every_set_variable_and_the_docs() {
        let text = note(env(&[
            ("HTTPS_PROXY", "http://p:3128"),
            ("all_proxy", "socks5://s"),
        ]))
        .expect("a note");
        assert!(text.starts_with("HTTPS_PROXY, all_proxy is set"), "{text}");
        assert!(text.contains("docs/mcp"), "{text}");
        assert!(
            !text.contains("p:3128"),
            "the note must not print the proxy value: {text}"
        );
    }

    #[test]
    fn each_variable_alone_gives_a_note_that_names_it() {
        for name in PROXY_VARS {
            let text = note(env(&[(name, "http://p")])).expect("a note");
            assert!(text.starts_with(&format!("{name} is set")), "{text}");
        }
    }
}
