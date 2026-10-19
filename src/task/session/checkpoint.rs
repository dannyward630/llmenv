//! The `session start` checkpoint: which open sessions the caller owns, and the refusal that
//! lists them.

use super::{EngineIdentity, Session, idle_display};

/// The one listed session that `owner` started, or `None` when it started none or more than one.
/// After a `/clear` or a compaction the choice is then clear, so `Auto` resumes it (#2904).
pub(super) fn only_own_session<'a>(
    mut existing: impl Iterator<Item = &'a Session>,
    owner: &EngineIdentity,
) -> Option<&'a Session> {
    let mut own = existing
        .by_ref()
        .filter(|s| !ownership_note(s, owner).is_empty());
    match (own.next(), own.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// Build the `session start` checkpoint error message: lists every existing
/// open same-project session with enough detail (id, name, description,
/// idle duration, and whether `owner` started it) that the agent or a human
/// can decide `--resume`, `--replace`, or `--new` without needing to inspect
/// anything further.
pub(super) fn checkpoint_error(existing: &[Session], owner: &EngineIdentity) -> String {
    let lines: Vec<String> = existing
        .iter()
        .map(|s| {
            let idle = idle_display(&s.last_activity);
            format!(
                "  - {} ({}){} — idle {idle}{}",
                s.id,
                s.name.as_deref().unwrap_or("unnamed"),
                s.description
                    .as_deref()
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default(),
                ownership_note(s, owner),
            )
        })
        .collect();
    let yours = existing
        .iter()
        .filter(|s| !ownership_note(s, owner).is_empty())
        .count();
    // `--replace` abandons every listed session, so it is only offered as the
    // way to drop "yours" when every listed session is yours.
    let hint = if yours == 0 {
        String::new()
    } else if yours == existing.len() {
        "\nEvery listed session is yours: pass --resume <id> to continue one, or --replace \
         to drop them all. Use --new only for a second, parallel window."
            .to_string()
    } else {
        format!(
            "\nA session marked as yours holds your own earlier work: pass --resume <id> to \
             continue it, or close it with `llmenv task session finish <id>`. --replace \
             abandons all {} listed sessions, including the ones that are not yours.",
            existing.len()
        )
    };
    format!(
        "session(s) already open for this project:\n{}\n\
         pass one of --resume <id>, --replace, or --new{hint}",
        lines.join("\n")
    )
}

/// The checkpoint's note on whether `owner` started `session`. The same
/// engine process with a different conversation id is the state after a
/// `/clear` or a compaction (#2365).
fn ownership_note(session: &Session, owner: &EngineIdentity) -> &'static str {
    let same_conversation = owner.session_id.is_some()
        && session.owner_session.as_deref() == owner.session_id.as_deref();
    let same_process = owner.pid.is_some() && session.owner_pid == owner.pid;
    if same_conversation {
        " — yours: started by this conversation"
    } else if same_process {
        " — yours: started by this engine process, most likely before a /clear or a compaction"
    } else {
        ""
    }
}
