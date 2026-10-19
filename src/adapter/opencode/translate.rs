//! The translation of Claude Code command and agent markdown to opencode's frontmatter.

/// Split markdown source into optional frontmatter and body.
///
/// Returns `(parsed_frontmatter_or_None, body_text)`. The body text is the
/// content after the closing `---` delimiter (leading whitespace trimmed).
pub(super) fn split_frontmatter(source: &str) -> (Option<serde_yaml::Value>, &str) {
    let s = source.trim_start();
    if !s.starts_with("---") {
        return (None, source);
    }
    let after_opener = &s[3..];

    // Find the closing `\n---` (opener + newline + closing delimiter).
    if let Some(end) = after_opener.find("\n---") {
        // `end` is the position of `\n` before the closing `---`. YAML
        // content is between the opening `---` and that `\n`; strip_prefix
        // (rather than a fixed `[1..]` byte slice) drops the newline right
        // after the opener without risking a non-UTF-8-boundary panic when
        // there isn't one (e.g. malformed input like "---é\n---").
        let content = &after_opener[..end];
        let fm_raw = content.strip_prefix('\n').unwrap_or(content);
        let body = &after_opener[(end + 4)..];
        let fm = match serde_yaml::from_str(fm_raw) {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::error!("failed to parse frontmatter YAML (treated as absent): {e}");
                None
            }
        };
        (fm, body.trim_start())
    } else {
        (None, source)
    }
}

/// Translate a command markdown file from Claude frontmatter to opencode format.
///
/// Keeps only `description` from the frontmatter. Drops `model` and
/// `allowed_tools` with a warning — those are Claude-specific concepts that
/// opencode does not support for commands.
pub(super) fn translate_command_md(source: &str, _name: &str) -> anyhow::Result<String> {
    let (fm, body) = split_frontmatter(source);
    let mut new_fm = serde_yaml::Mapping::new();

    if let Some(serde_yaml::Value::Mapping(ref map)) = fm {
        if let Some(desc) = map.get(serde_yaml::Value::String("description".into())) {
            new_fm.insert("description".into(), desc.clone());
        }
        let dropped: &[&str] = &["model", "allowed_tools"];
        for key in dropped {
            if map.contains_key(serde_yaml::Value::String((*key).into())) {
                eprintln!(
                    "warning: opencode adapter does not support '{key}' in \
                     command frontmatter — dropping this field"
                );
            }
        }
    }

    if new_fm.is_empty() {
        Ok(body.to_string())
    } else {
        let fm_yaml = serde_yaml::to_string(&new_fm)?;
        Ok(format!("---\n{}---\n{}", fm_yaml, body))
    }
}

/// Frontmatter keys that produce no drop warning: the keys `translate_agent_md` copies, plus
/// `name`, because opencode takes the agent name from the file name.
pub(super) const OPENCODE_AGENT_KEYS: &[&str] =
    &["description", "model", "tools", "allowed_tools", "name"];

/// Source frontmatter keys that opencode has no field for (#2150), in source order. A key that
/// is not a string (`1: x`) is rendered as YAML, so it is reported instead of dropped silently.
pub(super) fn dropped_agent_keys(map: &serde_yaml::Mapping) -> Vec<String> {
    map.keys()
        .filter(|key| {
            key.as_str()
                .is_none_or(|k| !OPENCODE_AGENT_KEYS.contains(&k))
        })
        .map(|key| match key.as_str() {
            Some(k) => k.to_owned(),
            None => serde_yaml::to_string(key)
                .unwrap_or_default()
                .trim()
                .to_owned(),
        })
        .collect()
}

/// Translate an agent markdown file from Claude frontmatter to opencode format.
///
/// Keeps `description`, `model`, `tools` / `allowed_tools`, and adds
/// `mode: subagent` so opencode runs the agent as a sub-process.
pub(super) fn translate_agent_md(source: &str, _name: &str) -> anyhow::Result<String> {
    let (fm, body) = split_frontmatter(source);
    let mut new_fm = serde_yaml::Mapping::new();

    if let Some(serde_yaml::Value::Mapping(ref map)) = fm {
        let keep: &[&str] = &["description", "model"];
        for key in keep {
            if let Some(val) = map.get(serde_yaml::Value::String((*key).into())) {
                new_fm.insert((*key).into(), val.clone());
            }
        }
        for tool_key in &["tools", "allowed_tools"] {
            if let Some(val) = map.get(serde_yaml::Value::String((*tool_key).into())) {
                new_fm.insert((*tool_key).into(), val.clone());
            }
        }

        for key in dropped_agent_keys(map) {
            // Agent files come from third-party plugins, so a key can hold terminal escapes.
            let key = crate::util::escape_control(&key);
            eprintln!(
                "warning: opencode adapter does not support '{key}' in \
                 agent frontmatter — dropping this field"
            );
        }
    }

    new_fm.insert("mode".into(), serde_yaml::Value::String("subagent".into()));
    let fm_yaml = serde_yaml::to_string(&new_fm)?;
    Ok(format!("---\n{}---\n{}", fm_yaml, body))
}
