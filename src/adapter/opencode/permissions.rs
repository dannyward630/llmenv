//! opencode 2.x permission rules (#2846).
//!
//! opencode 2.x reads `permissions` as one ordered list of `{action, resource, effect}` rules,
//! and applies the last rule that matches (`packages/core/src/permission.ts`, `evaluate`).

use std::collections::BTreeMap;

/// One rule of the `permissions` list in `opencode.json`.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[cfg_attr(test, derive(serde::Deserialize, PartialEq, Debug))]
pub(super) struct Rule {
    /// The tool action, such as `shell` or `edit`. `*` matches every action.
    pub(super) action: String,
    /// The input pattern, such as a command or a path. `*` matches every input.
    pub(super) resource: String,
    /// `allow`, `ask`, or `deny`.
    pub(super) effect: String,
}

/// The `permissions` list for `opencode.json`, or `None` when no rule applies to opencode.
///
/// # Errors
/// A `native_permissions.opencode` rule is malformed, or two rules resolve against the user's
/// intent under opencode's last-match order (see [`check_rule_ordering`]).
pub(super) fn render(
    perms: &crate::config::Permissions,
    native_perms: Option<&crate::config::NativePermissionRules>,
) -> anyhow::Result<Option<Vec<Rule>>> {
    let mut permission_map: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();

    // Convert a PermissionRule into (opencode_tool, pattern) pairs.
    // Rules with a pattern use it; rules with paths use each path as a pattern;
    // bare rules (no pattern, no paths) wildcard-match everything for the tool.
    // Empty when the tool has no opencode equivalent (see opencode_tool_name).
    //
    // #1371: rendering is silent about an unmapped tool. It used to `eprintln!`
    // here — once per *rule*, so a repeated tool name warned repeatedly, in the
    // middle of the progress output where it read as though the previous
    // adapter had failed. The two cases it conflated are reported properly
    // elsewhere now: a name outside the neutral vocabulary is a config error
    // that `cli::warn_dead_config` reports once, and a known tool that this
    // engine has no analog for is an adapter particularity documented in
    // `website/docs/engines.md` and listed by `doctor` — not something to
    // reprint on every shell prompt.
    fn rule_to_patterns(rule: &crate::config::PermissionRule) -> Vec<(String, String)> {
        let Some(tool) = opencode_tool_name(&rule.tool) else {
            return Vec::new();
        };
        if let Some(pat) = &rule.pattern {
            vec![(tool.to_string(), pat.clone())]
        } else if !rule.paths.is_empty() {
            rule.paths
                .iter()
                .map(|p| (tool.to_string(), p.clone()))
                .collect()
        } else {
            vec![(tool.to_string(), "*".to_string())]
        }
    }

    // Parse a native string like "bash(otool *)" or bare "bash" back into
    // (opencode_tool, pattern). A bare tool name wildcard-matches everything.
    // Anything else missing a balanced, non-empty `(pattern)` is a malformed
    // rule and must error rather than silently fall back to a wildcard —
    // a typo like "bash(" would otherwise wildcard-allow all bash commands.
    //
    // #1326 (security-audit): unlike `rule_to_patterns`, this does NOT run
    // the tool portion through `opencode_tool_name`. `native_permissions.
    // opencode` is the escape hatch for authoring opencode's *own* native
    // vocabulary directly — its own permission keys (`lsp`, `skill`,
    // `question`, `external_directory`, `doom_loop`, or a bare `"*"`
    // deny-all baseline) have no neutral equivalent at all, so routing
    // them through the neutral-name allowlist silently dropped every
    // native rule that was not a mapped PascalCase name, including a plain
    // lowercase shell deny, which turned a working deny into a prompt. The name
    // is lowercased, and a 1.x key gets its 2.x action (`normalize_action`).
    // A permission key with whitespace in it can never match a tool name,
    // so it would render as a rule that silently does nothing (#1328).
    fn reject_spaced_tool(tool: &str, rule: &str, action: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            !tool.contains(char::is_whitespace),
            "native_permissions.opencode.{action}: rule {rule:?} has whitespace inside its \
             tool name ({tool:?}) — opencode would never match it against a tool, so the \
             rule could not take effect. Write the tool name without spaces."
        );
        Ok(())
    }

    fn parse_native_rule(s: &str, action: &str) -> anyhow::Result<(String, String)> {
        match (s.find('('), s.rfind(')')) {
            (None, None) => {
                let tool = normalize_action(&s.trim().to_ascii_lowercase());
                anyhow::ensure!(
                    !tool.is_empty(),
                    "native_permissions.opencode.{action}: empty rule string"
                );
                reject_spaced_tool(&tool, s, action)?;
                Ok((tool, "*".to_string()))
            }
            (Some(start), Some(end)) if start < end => {
                // #1328: an untrimmed name renders an action like `"webfetch "`, which never
                // matches the real tool, so the rule could never take effect.
                let tool = normalize_action(&s[..start].trim().to_ascii_lowercase());
                let pattern = &s[start + 1..end];
                anyhow::ensure!(
                    !tool.is_empty(),
                    "native_permissions.opencode.{action}: rule {s:?} has no tool name before '('"
                );
                reject_spaced_tool(&tool, s, action)?;
                anyhow::ensure!(
                    !pattern.is_empty(),
                    "native_permissions.opencode.{action}: rule {s:?} has an empty pattern '()'"
                );
                // Text after the closing `)` would be dropped, so the rule would cover less or
                // more than the user wrote.
                anyhow::ensure!(
                    s[end + 1..].trim().is_empty(),
                    "native_permissions.opencode.{action}: rule {s:?} has text after its closing \
                     ')'. Write the rule as tool(pattern)."
                );
                Ok((tool, pattern.to_string()))
            }
            _ => anyhow::bail!(
                "native_permissions.opencode.{action}: rule {s:?} has unbalanced parentheses"
            ),
        }
    }

    // Insert (tool, pattern) pairs into the per-tool map.
    // Deny overrides ask overrides allow for the same tool+pattern
    // (last-write-wins).
    fn insert_patterns(
        map: &mut BTreeMap<String, BTreeMap<String, String>>,
        iter: impl Iterator<Item = (String, String)>,
        action: &str,
    ) {
        for (tool, pattern) in iter {
            map.entry(tool)
                .or_default()
                .insert(pattern, action.to_string());
        }
    }

    // Insertion order: interleaved by action tier (allow, then ask, then
    // deny), structured before native within each tier. This guarantees
    // deny always wins for a given tool+pattern regardless of which
    // source (structured config or native_permissions) wrote it — the
    // deny tier is inserted last across both sources. A native wildcard
    // `*` for a tool still shadows all structured patterns for that tool
    // within the same tier; use one category (structured OR native) per
    // tool to avoid that shadowing.
    for (action, structured, native) in [
        ("allow", &perms.allow, native_perms.map(|n| &n.allow)),
        ("ask", &perms.ask, native_perms.map(|n| &n.ask)),
        ("deny", &perms.deny, native_perms.map(|n| &n.deny)),
    ] {
        insert_patterns(
            &mut permission_map,
            structured.iter().flat_map(rule_to_patterns),
            action,
        );
        if let Some(n) = native {
            let native_patterns: Vec<(String, String)> = n
                .iter()
                .map(|s| parse_native_rule(s, action))
                .collect::<anyhow::Result<_>>()?;
            insert_patterns(&mut permission_map, native_patterns.into_iter(), action);
        }
    }

    if permission_map.is_empty() {
        return Ok(None);
    }
    check_rule_ordering(&permission_map)?;
    // The list keeps the order that `check_rule_ordering` checked.
    let rules = permission_map
        .into_iter()
        .flat_map(|(action, patterns)| {
            patterns.into_iter().map(move |(resource, effect)| Rule {
                action: action.clone(),
                resource,
                effect,
            })
        })
        .collect();
    Ok(Some(rules))
}

/// The opencode 2.x action for an opencode 1.x permission key, as opencode's own
/// `normalizeAction` maps it (`packages/core/src/v1/config/migrate.ts`). Without this, a
/// `native_permissions.opencode` rule on `bash` and a rule on `shell` look like two keys to
/// [`check_rule_ordering`], but opencode applies both to the one `shell` action.
fn normalize_action(key: &str) -> String {
    match key {
        "write" | "patch" => "edit",
        "task" => "subagent",
        "bash" => "shell",
        other => other,
    }
    .to_string()
}

/// opencode's action for a neutral tool name (Claude Code's vocabulary, such as `Bash`), or
/// `None` when opencode 2.x has no such tool. The table is the opencode column of
/// [`crate::adapter::tools`] (#1371, #2846). A guessed lowercase name is not used, because a rule
/// for a tool that does not exist never matches.
pub(super) fn opencode_tool_name(neutral: &str) -> Option<&'static str> {
    crate::adapter::tools::opencode_key(neutral)
}

/// Reject a rule pair whose rendered order inverts the user's intent.
///
/// opencode resolves a request with `findLast` over the `permissions` list, and wildcard-matches
/// both the action against the tool name and the resource against the input
/// (`packages/core/src/permission.ts`, `evaluate`). So the last rule that matches wins. That is
/// what the user meant only when it is the *narrower* of the two; otherwise the earlier rule
/// silently stops applying where they overlap.
///
/// [`render`] writes the rules in the order of this map, so iterating the map reproduces the
/// list opencode evaluates. Comparing across actions as well as within one is what makes a
/// wildcard action (`*`, authored via `native_permissions.opencode`) participate (#1344).
///
/// Only pairs with differing effects matter — when both say `deny`, whichever wins gives the
/// same answer.
pub(super) fn check_rule_ordering(
    permission_map: &BTreeMap<String, BTreeMap<String, String>>,
) -> anyhow::Result<()> {
    // (key, pattern, action) in rendered order — the order opencode sees.
    let rules: Vec<(&str, &str, &str)> = permission_map
        .iter()
        .flat_map(|(tool, patterns)| {
            patterns
                .iter()
                .map(move |(pattern, action)| (tool.as_str(), pattern.as_str(), action.as_str()))
        })
        .collect();

    for (i, (early_tool, early_pat, early_action)) in rules.iter().enumerate() {
        for (late_tool, late_pat, late_action) in rules.iter().skip(i + 1) {
            let inverts = late_action != early_action
                && patterns_overlap(early_tool, late_tool)
                && patterns_overlap(early_pat, late_pat)
                && !(pattern_subsumes(early_tool, late_tool)
                    && pattern_subsumes(early_pat, late_pat));
            if !inverts {
                continue;
            }
            anyhow::bail!(
                "opencode: permission rules {early} ({early_action}) and {late} ({late_action}) \
                 both match some tool and input, and {late} sorts last so opencode applies it \
                 wherever they overlap — {early} would not take effect there. {remedy}",
                early = render_rule(early_tool, early_pat),
                late = render_rule(late_tool, late_pat),
                remedy = ordering_remedy(early_tool, early_pat, late_tool, late_pat),
            );
        }
    }
    Ok(())
}

/// How to rewrite an inverted pair so opencode resolves it as intended.
///
/// When the two rules sit on *different* permission keys the earlier one is
/// almost always a wildcard key, and there is no way to narrow the later rule
/// out of its way — opencode has no exclusion syntax and llmenv sorts the keys,
/// so the user cannot reorder around it either. The fix that does work is to
/// restate the earlier rule on the later rule's key, where it sorts after the
/// broad pattern and wins.
fn ordering_remedy(early_tool: &str, early_pat: &str, late_tool: &str, late_pat: &str) -> String {
    if early_tool == late_tool {
        return format!(
            "Rewrite {late} so it only covers what {early} does not, or give the two the \
             same action.",
            late = render_rule(late_tool, late_pat),
            early = render_rule(early_tool, early_pat),
        );
    }
    format!(
        "Move {early} onto the `{late_tool}` key so it sorts after {late} — e.g. \
         `native_permissions.opencode` rule {moved} — or give the two the same action.",
        early = render_rule(early_tool, early_pat),
        late = render_rule(late_tool, late_pat),
        moved = render_rule(late_tool, early_pat),
    )
}

/// A rule in opencode's own `tool(pattern)` syntax.
///
/// That is the authoring syntax for `native_permissions.opencode`; a rule that
/// came from `capabilities.permissions` is shown the same way, since this is
/// how it lands in the generated config.
fn render_rule(tool: &str, pattern: &str) -> String {
    if pattern == "*" {
        format!("`{tool}`")
    } else {
        format!("`{tool}({pattern})`")
    }
}

/// True when some input matches both patterns.
///
/// Decides `L(a) ∩ L(b) ≠ ∅` exactly for opencode's `*`/`?` globs: `*` stands
/// for any run of characters and `?` for exactly one, so two patterns share an
/// input when their symbols can be aligned. A pattern ending in `" *"` also
/// matches its bare prefix, so each side is tested in both spellings.
pub(super) fn patterns_overlap(a: &str, b: &str) -> bool {
    fn spellings(pattern: &str) -> Vec<String> {
        let pattern = pattern.replace('\\', "/");
        match pattern.strip_suffix(" *") {
            Some(prefix) => vec![prefix.to_string(), pattern.clone()],
            None => vec![pattern],
        }
    }
    let (a, b) = (spellings(a), spellings(b));
    a.iter().any(|x| b.iter().any(|y| globs_intersect(x, y)))
}

/// `L(a) ∩ L(b) ≠ ∅` for two anchored `*`/`?` globs, by dynamic programming
/// over suffix pairs.
fn globs_intersect(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    // reachable[i][j] — some string is generated by both `a[i..]` and `b[j..]`.
    let rest_all_stars = |p: &[char]| p.iter().all(|c| *c == '*');
    let mut reachable = vec![vec![false; b.len() + 1]; a.len() + 1];
    for i in (0..=a.len()).rev() {
        for j in (0..=b.len()).rev() {
            reachable[i][j] = match (a.get(i), b.get(j)) {
                (None, None) => true,
                // One side is exhausted: the other must be able to match "".
                (None, Some(_)) => rest_all_stars(&b[j..]),
                (Some(_), None) => rest_all_stars(&a[i..]),
                // A `*` either matches nothing here, or absorbs one more
                // character that the other side also has to produce.
                (Some('*'), _) => reachable[i + 1][j] || reachable[i][j + 1],
                (_, Some('*')) => reachable[i][j + 1] || reachable[i + 1][j],
                // Otherwise both sides consume exactly one character, which
                // must be compatible.
                (Some(x), Some(y)) => (*x == '?' || *y == '?' || x == y) && reachable[i + 1][j + 1],
            };
        }
    }
    reachable[0][0]
}

/// True when `broad` appears to match everything `narrow` matches.
///
/// Substitutes a sentinel run for each of `narrow`'s `*` so it can be tested as
/// a concrete string. One length is not enough: a run of `?` in `broad` can
/// stand in for a short run, and a `*` in `narrow` also stands for *no*
/// characters at all, which a `broad` like `"*?"` cannot match. So `broad` has
/// to match every probe length — zero, one, and past `broad`'s own `?` count.
///
/// This is a witness test, not a containment proof. It is used only to *exempt*
/// a pattern pair from the ordering check, so a false negative just costs a
/// spurious error while the rendered config stays correct — whereas a false
/// positive would silently skip a real inversion, which is what the probe
/// lengths guard against.
pub(super) fn pattern_subsumes(broad: &str, narrow: &str) -> bool {
    // U+0001 can't appear in a shell command or URL, so no literal character in
    // `broad` can match it.
    const SENTINEL: char = '\u{1}';
    let widest = broad.chars().filter(|c| *c == '?').count() + 1;
    [0, 1, widest].iter().all(|run| {
        let mut witness = String::new();
        for c in narrow.chars() {
            match c {
                '*' => witness.extend(std::iter::repeat_n(SENTINEL, *run)),
                '?' => witness.push(SENTINEL),
                other => witness.push(other),
            }
        }
        wildcard_matches(broad, &witness)
    })
}

/// Match `input` against an opencode permission pattern.
///
/// Mirrors opencode's `Wildcard.match` (`packages/core/src/util/wildcard.ts`):
/// `*` spans any run of characters, `?` spans exactly one, backslashes
/// normalize to `/`, and a pattern ending in `" *"` also matches the bare
/// prefix (so `"git *"` covers a plain `"git"`).
pub(super) fn wildcard_matches(pattern: &str, input: &str) -> bool {
    let normalized: Vec<char> = input.replace('\\', "/").chars().collect();
    let pattern = pattern.replace('\\', "/");
    if let Some(prefix) = pattern.strip_suffix(" *")
        && glob_matches(prefix, &normalized)
    {
        return true;
    }
    glob_matches(&pattern, &normalized)
}

/// Anchored glob match over `*` and `?`, backtracking on the most recent `*`.
pub(super) fn glob_matches(pattern: &str, input: &[char]) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let (mut p, mut i) = (0_usize, 0_usize);
    let (mut star, mut resume) = (None, 0_usize);
    while i < input.len() {
        match pat.get(p) {
            Some('*') => {
                star = Some(p);
                resume = i;
                p += 1;
            }
            Some('?') => {
                p += 1;
                i += 1;
            }
            Some(c) if input.get(i) == Some(c) => {
                p += 1;
                i += 1;
            }
            _ => match star {
                Some(s) => {
                    p = s + 1;
                    resume += 1;
                    i = resume;
                }
                None => return false,
            },
        }
    }
    pat.iter().skip(p).all(|c| *c == '*')
}
