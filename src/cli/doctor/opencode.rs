//! Doctor checks for config that opencode does not apply as Claude Code does.

use crate::config::Capabilities;
use std::path::Path;

/// A permission rule that is dead config under opencode (#838).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::cli) struct ColonPrefixRule {
    /// `"allow"`, `"ask"`, or `"deny"` — the tier the rule was declared in.
    pub tier: &'static str,
    /// `"<tool>(<pattern>)"`, as the user would recognize it.
    pub rule: String,
}

impl ColonPrefixRule {
    /// The diagnostic for this rule. A dead `deny` fails **open** — the rule the
    /// user wrote to block something doesn't block it — so it gets stronger
    /// wording than a dead `allow`, which merely fails closed into a prompt.
    pub fn message(&self) -> String {
        let Self { tier, rule } = self;
        let consequence = if *tier == "deny" {
            "so the deny is NOT enforced there — whatever it was meant to block is left \
             to opencode's default"
        } else {
            "so the grant never applies there"
        };
        format!(
            "permission rule {rule} ({tier}) uses Claude Code's colon-prefix syntax, which \
             opencode matches as a literal glob, {consequence}. Use a space-separated pattern \
             (e.g. `git commit *`) for a rule both engines honour, or move the Claude-only form \
             to `native_permissions.claude_code`"
        )
    }
}

/// Returns every neutral permission rule whose pattern uses Claude Code's
/// colon-prefix syntax, for a config where opencode is also materialized (#838).
///
/// opencode matches a permission pattern as a plain glob against the whole
/// command string, so `git commit:*` matches nothing there. Only the
/// engine-neutral `permissions` block is checked; `native_permissions.claude_code`
/// is Claude-scoped on purpose and correct as written.
///
/// Takes merged `capabilities` so rules contributed by a `bundle.yaml` are
/// covered — a dead `deny` shipped in a shared bundle is the case most worth
/// catching. `opencode_active` is passed in rather than probed so the check is
/// testable without depending on the host's `PATH`.
pub(in crate::cli) fn claude_only_colon_permission_patterns(
    capabilities: &Capabilities,
    opencode_active: bool,
) -> Vec<ColonPrefixRule> {
    if !opencode_active {
        return Vec::new();
    }
    let perms = &capabilities.permissions;
    [
        ("allow", &perms.allow),
        ("ask", &perms.ask),
        ("deny", &perms.deny),
    ]
    .into_iter()
    .flat_map(|(tier, rules)| rules.iter().map(move |rule| (tier, rule)))
    .filter_map(|(tier, rule)| {
        let pattern = rule.pattern.as_deref()?;
        super::uses_colon_prefix_syntax(pattern).then(|| ColonPrefixRule {
            tier,
            rule: format!(
                "{}({})",
                crate::util::display_safe(&rule.tool),
                crate::util::display_safe(pattern)
            ),
        })
    })
    .collect()
}

/// The warning for a `~/.agents/skills` folder under `home`, or `None` when it is absent.
/// opencode 2.x loads every skill in it on every session, and no setting turns that folder off
/// (#2843). The `~/.claude` roots have a switch, which the adapter sets.
pub(super) fn agents_skills_warning(home: &Path) -> Option<String> {
    let dir = home.join(".agents").join("skills");
    dir.is_dir().then(|| {
        format!(
            "{} exists. opencode 2.x loads every skill in it on every session, outside llmenv's \
             scope rules, and has no setting to turn it off. Fix: move these skills into an \
             llmenv bundle, or remove the folder",
            dir.display()
        )
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn an_agents_skills_folder_is_named_with_the_fix() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".agents/skills")).unwrap();
        let msg = agents_skills_warning(home.path()).unwrap();
        assert!(msg.contains(".agents/skills"), "{msg}");
        assert!(msg.contains("Fix:"), "{msg}");
    }

    #[test]
    fn no_folder_or_a_file_gives_no_warning() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(agents_skills_warning(home.path()), None);
        std::fs::create_dir_all(home.path().join(".agents")).unwrap();
        std::fs::write(home.path().join(".agents/skills"), "").unwrap();
        assert_eq!(agents_skills_warning(home.path()), None);
    }
}
