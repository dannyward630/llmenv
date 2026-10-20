//! Quoting for text that a user pastes into a shell.

use std::path::Path;

/// `s` in single quotes, with each embedded single quote closed, escaped, and reopened. The
/// shell expands nothing inside, so the text reaches the command as it is.
#[must_use]
pub fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `path` as one shell word, so a command in a message can be pasted as it is.
#[must_use]
pub fn shell_word(path: &Path) -> String {
    let text = path.display().to_string();
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@%".contains(c));
    if plain {
        text
    } else if text.is_empty() {
        // An empty word would vanish from the pasted command, so it is written as a quoted empty.
        "''".to_string()
    } else {
        single_quote(&text)
    }
}
