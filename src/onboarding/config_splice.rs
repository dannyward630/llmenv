//! Add entries to `config.yaml` by inserting text, never by re-serializing the file (#2788).
//!
//! A re-serialized file loses its comments and its key order. The insert changes no existing
//! line, except a key whose value is empty (`null`, `~`, `[]`, `{}`), which gets its block. The
//! result must parse to the old configuration plus the additions, or the splice is refused.
//! Design: docs/design/issue-2773-onboarding.md ("The config.yaml change").

use anyhow::{Context as _, bail, ensure};
use serde_yaml::{Mapping, Value};

/// One addition to `config.yaml`: list items, or new keys of a mapping, at a top-level `key` or
/// at one `sub` key below it (such as `features.memory`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Addition {
    Items {
        key: &'static str,
        sub: Option<&'static str>,
        items: Vec<Value>,
    },
    Keys {
        key: &'static str,
        sub: Option<&'static str>,
        keys: Mapping,
    },
}

impl Addition {
    fn key(&self) -> &'static str {
        match self {
            Self::Items { key, .. } | Self::Keys { key, .. } => key,
        }
    }

    fn sub(&self) -> Option<&'static str> {
        match self {
            Self::Items { sub, .. } | Self::Keys { sub, .. } => *sub,
        }
    }

    /// The added entries as one value: a list, or a mapping.
    fn value(&self) -> Value {
        match self {
            Self::Items { items, .. } => Value::Sequence(items.clone()),
            Self::Keys { keys, .. } => Value::Mapping(keys.clone()),
        }
    }

    /// The added entries, wrapped in the sub key when there is one.
    fn under_key(&self) -> Value {
        match self.sub() {
            Some(sub) => {
                let mut map = Mapping::new();
                map.insert(Value::from(sub), self.value());
                Value::Mapping(map)
            }
            None => self.value(),
        }
    }

    fn name(&self) -> String {
        match self.sub() {
            Some(sub) => format!("{}.{sub}", self.key()),
            None => self.key().to_string(),
        }
    }
}

/// One change: `removed` lines replaced by `added` lines, which start at 1-based `line` of the
/// new text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    line: usize,
    removed: Vec<String>,
    added: Vec<String>,
}

/// The changes as diff lines: `-` for a removed line, `+` for an added one.
fn render(hunks: &[Hunk]) -> String {
    let mut out = String::new();
    for hunk in hunks {
        out.push_str(&format!(
            "@@ line {} of the new config.yaml @@\n",
            hunk.line
        ));
        hunk.removed
            .iter()
            .for_each(|l| out.push_str(&format!("- {l}\n")));
        hunk.added
            .iter()
            .for_each(|l| out.push_str(&format!("+ {l}\n")));
    }
    out
}

/// Insert `additions` into the `config.yaml` text `old`, one after the other. Returns the new
/// text and the change as diff lines.
///
/// # Errors
/// A target has a shape that the insert does not support (a flow-style list or a scalar), or
/// the result does not parse to the old configuration plus the additions. The error gives the
/// lines to add by hand.
pub(crate) fn splice(old: &str, additions: &[Addition]) -> anyhow::Result<(String, String)> {
    let eol = line_ending(old)?;
    let mut lines: Vec<String> = old.lines().map(str::to_string).collect();
    let mut hunks: Vec<Hunk> = Vec::new();
    for addition in additions {
        let hunk = insert(&mut lines, addition).with_context(|| manual_fix(addition))?;
        // An insert at or above an earlier hunk moves that hunk down in the new text.
        let grew = hunk.added.len().saturating_sub(hunk.removed.len());
        for earlier in hunks.iter_mut().filter(|h| h.line >= hunk.line) {
            earlier.line = earlier.line.saturating_add(grew);
        }
        hunks.push(hunk);
    }
    let mut text = lines.join(eol);
    text.push_str(eol);
    verify(old, &text, additions).with_context(|| {
        additions
            .iter()
            .map(manual_fix)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    Ok((text, render(&hunks)))
}

/// The line ending of `text`, so the inserted lines match it. A file with both endings is
/// refused, because a rewrite would change lines that the user did not approve.
fn line_ending(text: &str) -> anyhow::Result<&'static str> {
    let crlf = text.matches("\r\n").count();
    if crlf == 0 {
        return Ok("\n");
    }
    ensure!(
        crlf == text.matches('\n').count(),
        "config.yaml mixes CRLF and LF line endings. Fix: save it with one line ending, then \
         apply the plan again"
    );
    Ok("\r\n")
}

/// The new configuration must be the old one plus the additions, and nothing else.
fn verify(old: &str, new: &str, additions: &[Addition]) -> anyhow::Result<()> {
    let mut expected = parse(old).context("the current config.yaml does not parse")?;
    for addition in additions {
        add_to_value(&mut expected, addition)?;
    }
    let got = parse(new).context("the changed config.yaml does not parse")?;
    ensure!(
        got == expected,
        "the changed config.yaml does not hold exactly the old configuration plus the additions"
    );
    Ok(())
}

fn parse(text: &str) -> anyhow::Result<Value> {
    let value: Value = serde_yaml::from_str(text)?;
    Ok(if value.is_null() {
        Value::Mapping(Mapping::new())
    } else {
        value
    })
}

/// Apply `addition` to a parsed configuration, as the text insert must.
fn add_to_value(config: &mut Value, addition: &Addition) -> anyhow::Result<()> {
    let root = config
        .as_mapping_mut()
        .context("config.yaml is not a mapping")?;
    let mut slot = root
        .entry(Value::from(addition.key()))
        .or_insert(Value::Null);
    if let Some(sub) = addition.sub() {
        if slot.is_null() {
            *slot = Value::Mapping(Mapping::new());
        }
        let map = slot
            .as_mapping_mut()
            .with_context(|| format!("`{}` is not a mapping", addition.key()))?;
        slot = map.entry(Value::from(sub)).or_insert(Value::Null);
    }
    match addition {
        Addition::Items { items, .. } => {
            if slot.is_null() {
                *slot = Value::Sequence(Vec::new());
            }
            let seq = slot.as_sequence_mut().context("the target is not a list")?;
            seq.extend(items.iter().cloned());
        }
        Addition::Keys { keys, .. } => {
            if slot.is_null() {
                *slot = Value::Mapping(Mapping::new());
            }
            let map = slot
                .as_mapping_mut()
                .context("the target is not a mapping")?;
            map.extend(keys.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }
    Ok(())
}

/// The state of a key line: its block follows, its value is empty, or its value is inline.
#[derive(Debug, PartialEq, Eq)]
enum KeyValue {
    Block,
    Empty,
    Inline,
}

fn key_value(line: &str, key: &str) -> KeyValue {
    let rest = line.trim_start().strip_prefix(key).unwrap_or_default();
    let rest = rest.strip_prefix(':').unwrap_or_default();
    // A comment needs a space before `#`; a `#` inside a value is not one.
    let value = rest.split(" #").next().unwrap_or_default().trim();
    match value {
        "" if rest.trim_start().starts_with('#') || rest.trim().is_empty() => KeyValue::Block,
        "null" | "~" | "[]" | "{}" => KeyValue::Empty,
        _ => KeyValue::Inline,
    }
}

fn indent_of(line: &str) -> usize {
    line.len()
        .saturating_sub(line.trim_start_matches(' ').len())
}

fn is_content(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.is_empty() && !trimmed.starts_with('#')
}

/// The index of the line `key:` at `indent` in `lines[from..to]`.
fn find_key(lines: &[String], from: usize, to: usize, key: &str, indent: usize) -> Option<usize> {
    (from..to).find(|&i| {
        lines.get(i).is_some_and(|l| {
            indent_of(l) == indent
                && l.trim_start()
                    .strip_prefix(key)
                    .is_some_and(|r| r.starts_with(':'))
        })
    })
}

/// The end (exclusive) of the block of the key at `start` with `indent`: the first content line
/// that is not deeper, except a `- ` item at the same indent (an indentless list).
fn block_end(lines: &[String], start: usize, indent: usize) -> usize {
    let mut end = start.saturating_add(1);
    while let Some(line) = lines.get(end) {
        if is_content(line) {
            let deeper = indent_of(line) > indent;
            let item = indent_of(line) == indent && line.trim_start().starts_with("- ");
            if !(deeper || item) || line.starts_with("---") {
                break;
            }
        }
        end = end.saturating_add(1);
    }
    end
}

/// The line after the last content line of a block, so trailing comments stay below.
fn after_content(lines: &[String], start: usize, end: usize) -> usize {
    (start..end)
        .rev()
        .find(|&i| lines.get(i).is_some_and(|l| is_content(l)))
        .map_or(start.saturating_add(1), |i| i.saturating_add(1))
}

/// `value` as YAML lines, each indented by `indent` spaces.
fn yaml_lines(value: &Value, indent: usize) -> anyhow::Result<Vec<String>> {
    let text = serde_yaml::to_string(value).context("encoding the addition as YAML")?;
    let pad = " ".repeat(indent);
    Ok(text.lines().map(|l| format!("{pad}{l}")).collect())
}

/// `{name: value}` as YAML lines at `indent`.
fn keyed_lines(name: &str, value: Value, indent: usize) -> anyhow::Result<Vec<String>> {
    let mut map = Mapping::new();
    map.insert(Value::from(name), value);
    yaml_lines(&Value::Mapping(map), indent)
}

fn insert(lines: &mut Vec<String>, addition: &Addition) -> anyhow::Result<Hunk> {
    let key = addition.key();
    let Some(at) = find_key(lines, 0, lines.len(), key, 0) else {
        let new = keyed_lines(key, addition.under_key(), 0)?;
        return Ok(put(lines, lines.len(), 0, new));
    };
    let line = lines.get(at).cloned().unwrap_or_default();
    match key_value(&line, key) {
        KeyValue::Empty => Ok(put(
            lines,
            at,
            1,
            keyed_lines(key, addition.under_key(), 0)?,
        )),
        KeyValue::Inline => bail!("`{key}` holds an inline value, and only a block is supported"),
        KeyValue::Block => {
            let end = block_end(lines, at, 0);
            match addition.sub() {
                Some(sub) => insert_sub(lines, at, end, sub, addition),
                None => insert_entries(lines, at, end, 0, addition),
            }
        }
    }
}

/// Insert below the sub key `sub` of the block at `at..end`.
fn insert_sub(
    lines: &mut Vec<String>,
    at: usize,
    end: usize,
    sub: &str,
    addition: &Addition,
) -> anyhow::Result<Hunk> {
    let child = (at.saturating_add(1)..end)
        .find_map(|i| lines.get(i).filter(|l| is_content(l)).map(|l| indent_of(l)))
        .unwrap_or(2);
    let Some(sub_at) = find_key(lines, at.saturating_add(1), end, sub, child) else {
        let new = keyed_lines(sub, addition.value(), child)?;
        let pos = after_content(lines, at, end);
        return Ok(put(lines, pos, 0, new));
    };
    let line = lines.get(sub_at).cloned().unwrap_or_default();
    match key_value(&line, sub) {
        KeyValue::Empty => Ok(put(
            lines,
            sub_at,
            1,
            keyed_lines(sub, addition.value(), child)?,
        )),
        KeyValue::Inline => bail!("`{sub}` holds an inline value, and only a block is supported"),
        KeyValue::Block => {
            let sub_end = block_end(lines, sub_at, child);
            insert_entries(lines, sub_at, sub_end, child, addition)
        }
    }
}

/// Insert list items or mapping keys at the end of the block of the key at `at..end`, whose
/// key sits at `indent`.
fn insert_entries(
    lines: &mut Vec<String>,
    at: usize,
    end: usize,
    indent: usize,
    addition: &Addition,
) -> anyhow::Result<Hunk> {
    let first = (at.saturating_add(1)..end).find_map(|i| lines.get(i).filter(|l| is_content(l)));
    let child = first.map_or(indent.saturating_add(2), |l| indent_of(l));
    if let (Addition::Items { .. }, Some(l)) = (addition, first) {
        ensure!(
            l.trim_start().starts_with("- "),
            "the target is not a block list"
        );
    }
    let new = yaml_lines(&addition.value(), child)?;
    let pos = after_content(lines, at, end);
    Ok(put(lines, pos, 0, new))
}

/// Replace `remove` lines at `pos` with `new`, and record the change.
fn put(lines: &mut Vec<String>, pos: usize, remove: usize, new: Vec<String>) -> Hunk {
    let pos = pos.min(lines.len());
    let stop = pos.saturating_add(remove).min(lines.len());
    let removed: Vec<String> = lines.splice(pos..stop, new.iter().cloned()).collect();
    Hunk {
        line: pos.saturating_add(1),
        removed,
        added: new,
    }
}

/// The lines to add by hand for one addition.
fn manual_fix(addition: &Addition) -> String {
    let lines = keyed_lines(addition.key(), addition.under_key(), 0)
        .unwrap_or_else(|e| vec![format!("(the lines cannot be shown: {e:#})")]);
    format!(
        "llmenv cannot add to `{}` in config.yaml safely. Fix: add these lines to config.yaml by \
         hand, merged into the existing key:\n{}",
        addition.name(),
        lines.join("\n")
    )
}

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod tests;
