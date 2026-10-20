//! The note that a failed request gets when a proxy variable is set (#2884). The MCP client and
//! the umans usage fetch ignore proxies since #2484, so a user behind a required proxy otherwise
//! sees only a timeout.

use std::ffi::OsString;

/// The proxy variables that the HTTP stack reads, upper and lower case.
pub(crate) const PROXY_VARS: &[&str] = &[
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// The docs section that explains why llmenv requests skip the proxy.
const DOCS_URL: &str = "https://phaedrus1992.github.io/llmenv/docs/mcp#memory-backend-memory";

/// Add the proxy note to a failed send, when any proxy variable is set. The note goes last on
/// one line, so every display of the error keeps the request and the cause first, and a reason
/// cut to a length limit loses the note, not the cause.
pub fn with_proxy_note(error: anyhow::Error) -> anyhow::Error {
    append(error, note(|name| std::env::var_os(name)))
}

/// `error` as one line with `note` after it, or `error` unchanged when there is no note.
fn append(error: anyhow::Error, note: Option<String>) -> anyhow::Error {
    match note {
        Some(note) => anyhow::anyhow!("{error:#}; {note}"),
        None => error,
    }
}

/// The note that names each set proxy variable, or `None` when no variable is set. An empty
/// value counts as unset here, because an empty variable names no proxy that a user relies on.
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
        "{} is set, but llmenv sends this request without a proxy, so a server reachable only \
         through the proxy cannot answer; see {DOCS_URL}",
        set.join(", ")
    ))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use super::{PROXY_VARS, append, note};

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

    #[test]
    fn a_non_utf8_value_still_names_the_variable() {
        use std::os::unix::ffi::OsStringExt;
        let get = |name: &str| (name == "HTTP_PROXY").then(|| OsString::from_vec(vec![0xff]));
        assert!(note(get).expect("a note").starts_with("HTTP_PROXY is set"));
    }

    #[test]
    fn the_flat_error_keeps_the_request_and_cause_first() {
        let error = anyhow::anyhow!("connection refused").context("POST http://h/mcp for x");
        let text = format!(
            "{}",
            append(error, note(env(&[("HTTPS_PROXY", "http://p")])))
        );
        assert!(
            text.starts_with("POST http://h/mcp for x: connection refused; HTTPS_PROXY is set"),
            "{text}"
        );
        let plain = anyhow::anyhow!("x");
        assert_eq!(format!("{:#}", append(plain, None)), "x");
    }
}
