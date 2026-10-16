//! Map an llmenv hook event name to the native name the engine expects.
//!
//! The engine echoes `hookEventName` in its schema. When the engine's stdin does not parse,
//! the name is unknown from stdin, so the argv event name is the only source left (#2748).

/// Return the engine's native event name for an llmenv event, or `None` when the engine has no
/// native name for it.
///
/// A `None` result means the hook output carries an empty `hookEventName`. That is the
/// best the hook can do without a native name.
pub(super) fn engine_event_name(event: &str) -> Option<&'static str> {
    // Explicit, not derived from snake_case: `turn_start` has no native name of its own.
    match event {
        "session_start" => Some("SessionStart"),
        "session_end" => Some("SessionEnd"),
        "turn_start" | "user_prompt_submit" => Some("UserPromptSubmit"),
        "pre_tool_use" => Some("PreToolUse"),
        "post_tool_use" => Some("PostToolUse"),
        "post_tool_use_failure" => Some("PostToolUseFailure"),
        "notification" => Some("Notification"),
        "stop" => Some("Stop"),
        "subagent_start" => Some("SubagentStart"),
        "subagent_stop" => Some("SubagentStop"),
        "pre_compact" => Some("PreCompact"),
        "post_model_switch" => Some("PostModelSwitch"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::engine_event_name;

    #[test]
    fn maps_session_start_to_its_native_name() {
        assert_eq!(engine_event_name("session_start"), Some("SessionStart"));
    }

    #[test]
    fn maps_turn_start_to_the_prompt_submit_name() {
        // `turn_start` fires on each prompt, so the engine calls it UserPromptSubmit.
        assert_eq!(engine_event_name("turn_start"), Some("UserPromptSubmit"));
    }

    #[test]
    fn returns_none_for_an_event_without_a_native_name() {
        assert_eq!(engine_event_name("post_session"), None);
    }

    #[test]
    fn returns_none_for_an_unknown_or_empty_event() {
        assert_eq!(engine_event_name("nope"), None);
        assert_eq!(engine_event_name(""), None);
    }
}
