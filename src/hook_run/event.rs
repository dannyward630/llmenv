//! The llmenv hook events and their names. Each name is the `hook-run` argument.

use std::str::FromStr;

/// An engine-neutral lifecycle event. Adapters translate these to native hook
/// names when wiring them into agent config.
///
/// `SessionStart`/`TurnStart`/`SessionEnd` drive ICM memory recall/store (see
/// `dispatch`) and the baseline session log (see `handle_session_log`). The
/// rest drive per-turn session-log capture (see `event_to_log_kind`); they
/// carry no memory actions of their own — Claude's `UserPromptSubmit` native
/// hook fires both `TurnStart` (memory recall) and `UserPromptSubmit`
/// (session-log capture) as two separate handlers on the same event (see
/// adapter wiring, #382 Task 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// Session begins (Claude Code: `SessionStart`).
    SessionStart,
    /// A user prompt/turn begins (Claude Code: `UserPromptSubmit`).
    TurnStart,
    /// Session ends (Claude Code: `SessionEnd`).
    SessionEnd,
    /// Post-session consolidation hook (R5). Runs after SessionEnd to
    /// trigger reflective consolidation on the accumulated conversation.
    PostSession,
    /// The raw prompt submission (Claude Code: `UserPromptSubmit`).
    UserPromptSubmit,
    /// Before a tool call (Claude Code: `PreToolUse`).
    PreToolUse,
    /// After a tool call (Claude Code: `PostToolUse`).
    PostToolUse,
    /// A UI notification fired (Claude Code: `Notification`).
    Notification,
    /// The main agent finished responding (Claude Code: `Stop`).
    Stop,
    /// A subagent finished responding (Claude Code: `SubagentStop`).
    SubagentStop,
    /// About to compact the transcript (Claude Code: `PreCompact`).
    PreCompact,
    /// A batch of tool calls resolved (Claude Code: `PostToolBatch`).
    PostToolBatch,
    /// A tool call failed (Claude Code: `PostToolUseFailure`).
    PostToolUseFailure,
    /// A subagent is spawned or resumed (Claude Code: `SubagentStart`).
    SubagentStart,
    /// An `Agent` tool call is about to run (Claude Code: `PreToolUse` with the
    /// `^Agent$` matcher); queues the subagent task for adaptive recall (#2249).
    SubagentTask,
    /// The model changed (Claude Code: `PostModelSwitch`); updates the session's
    /// agent-config document (#2398). Emits nothing.
    PostModelSwitch,
}

/// Every event name that `hook-run` accepts. A test keeps this list in step with the parser, and
/// the `hook-run` help text lists the same names.
pub(crate) const HOOK_EVENT_NAMES: [&str; 16] = [
    "session_start",
    "turn_start",
    "session_end",
    "user_prompt_submit",
    "post_session",
    "pre_tool_use",
    "post_tool_use",
    "notification",
    "stop",
    "subagent_stop",
    "pre_compact",
    "post_tool_batch",
    "post_tool_use_failure",
    "subagent_start",
    "subagent_task",
    "post_model_switch",
];

impl FromStr for HookEvent {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "session_start" => Ok(HookEvent::SessionStart),
            "turn_start" => Ok(HookEvent::TurnStart),
            "session_end" => Ok(HookEvent::SessionEnd),
            "user_prompt_submit" => Ok(HookEvent::UserPromptSubmit),
            "post_session" => Ok(HookEvent::PostSession),
            "pre_tool_use" => Ok(HookEvent::PreToolUse),
            "post_tool_use" => Ok(HookEvent::PostToolUse),
            "notification" => Ok(HookEvent::Notification),
            "stop" => Ok(HookEvent::Stop),
            "subagent_stop" => Ok(HookEvent::SubagentStop),
            "pre_compact" => Ok(HookEvent::PreCompact),
            "post_tool_batch" => Ok(HookEvent::PostToolBatch),
            "post_tool_use_failure" => Ok(HookEvent::PostToolUseFailure),
            "subagent_start" => Ok(HookEvent::SubagentStart),
            "subagent_task" => Ok(HookEvent::SubagentTask),
            "post_model_switch" => Ok(HookEvent::PostModelSwitch),
            other => Err(anyhow::anyhow!(
                "unknown hook event '{other}' (expected {})",
                HOOK_EVENT_NAMES.join("|")
            )),
        }
    }
}

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            HookEvent::SessionStart => "session_start",
            HookEvent::TurnStart => "turn_start",
            HookEvent::SessionEnd => "session_end",
            HookEvent::UserPromptSubmit => "user_prompt_submit",
            HookEvent::PreToolUse => "pre_tool_use",
            HookEvent::PostToolUse => "post_tool_use",
            HookEvent::Notification => "notification",
            HookEvent::Stop => "stop",
            HookEvent::SubagentStop => "subagent_stop",
            HookEvent::PreCompact => "pre_compact",
            HookEvent::PostSession => "post_session",
            HookEvent::PostToolBatch => "post_tool_batch",
            HookEvent::PostToolUseFailure => "post_tool_use_failure",
            HookEvent::SubagentStart => "subagent_start",
            HookEvent::SubagentTask => "subagent_task",
            HookEvent::PostModelSwitch => "post_model_switch",
        };
        f.write_str(s)
    }
}

impl HookEvent {
    /// The engine's native name for this event, or `None` when the engine has no name for it.
    ///
    /// The match has no catch-all arm, so a new event fails to compile until it is named here.
    /// `turn_start` fires on each prompt, so the engine calls it `UserPromptSubmit`.
    pub(crate) fn engine_name(self) -> Option<&'static str> {
        match self {
            HookEvent::SessionStart => Some("SessionStart"),
            HookEvent::SessionEnd => Some("SessionEnd"),
            HookEvent::TurnStart | HookEvent::UserPromptSubmit => Some("UserPromptSubmit"),
            HookEvent::PreToolUse => Some("PreToolUse"),
            HookEvent::PostToolUse => Some("PostToolUse"),
            HookEvent::PostToolUseFailure => Some("PostToolUseFailure"),
            HookEvent::Notification => Some("Notification"),
            HookEvent::Stop => Some("Stop"),
            HookEvent::SubagentStart => Some("SubagentStart"),
            HookEvent::SubagentStop => Some("SubagentStop"),
            HookEvent::PreCompact => Some("PreCompact"),
            HookEvent::PostModelSwitch => Some("PostModelSwitch"),
            HookEvent::PostSession | HookEvent::PostToolBatch | HookEvent::SubagentTask => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HookEvent;

    #[test]
    fn turn_start_maps_to_the_engine_prompt_submit_name() {
        assert_eq!(HookEvent::TurnStart.engine_name(), Some("UserPromptSubmit"));
    }

    #[test]
    fn events_without_a_native_name_return_none() {
        assert_eq!(HookEvent::PostSession.engine_name(), None);
    }
}
