//! Shape checks for permission rules in the config (`permissions:`).

use super::ValidateError;
use crate::PermissionRule;

/// A pattern/path is only safe to wrap as `Tool(value)` if its own parentheses
/// balance — otherwise the wrapped string's parens don't balance either, and
/// engines that parse the rule by tracking paren depth (Claude Code, Crush)
/// silently skip the whole rule. `<(` from shell process substitution is the
/// common trigger (`bash <(curl *` has one unmatched `(`).
pub(super) fn has_balanced_parens(s: &str) -> bool {
    let mut depth: i32 = 0;
    let mut prev_was_escape = false;
    for c in s.chars() {
        if prev_was_escape {
            prev_was_escape = false;
            continue;
        }
        match c {
            '\\' => prev_was_escape = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// Validate a single permission rule's `pattern`/`paths` for balanced
/// parentheses. Returns an error with the given `context` label (e.g.
/// `"config.yaml: capabilities.permissions"` or `"bundle 'foo'"`) on failure —
/// see [`ValidateError::PermissionRuleUnbalancedParens`] for why this is
/// rejected rather than rendered as-is.
pub fn validate_permission_rule(context: &str, rule: &PermissionRule) -> Result<(), ValidateError> {
    if let Some(pattern) = &rule.pattern
        && !has_balanced_parens(pattern)
    {
        return Err(ValidateError::PermissionRuleUnbalancedParens {
            context: context.to_string(),
            tool: rule.tool.clone(),
            value: pattern.clone(),
        });
    }
    for path in &rule.paths {
        if !has_balanced_parens(path) {
            return Err(ValidateError::PermissionRuleUnbalancedParens {
                context: context.to_string(),
                tool: rule.tool.clone(),
                value: path.clone(),
            });
        }
    }
    Ok(())
}

/// Validate a single native permission rule string (already in `Tool(value)`
/// format) for balanced parentheses — same rationale as
/// [`validate_permission_rule`].
pub fn validate_permission_string(context: &str, raw: &str) -> Result<(), ValidateError> {
    if !has_balanced_parens(raw) {
        return Err(ValidateError::PermissionRuleUnbalancedParens {
            context: context.to_string(),
            tool: raw.to_string(),
            value: raw.to_string(),
        });
    }
    Ok(())
}
