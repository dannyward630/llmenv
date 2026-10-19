//! The one-line form of a failure reason for the health notice.

/// The most characters of a failure reason that the notice carries. A server controls part of
/// the text, such as an HTTP error body, and the notice goes into the agent's context.
pub(crate) const MAX_REASON_CHARS: usize = 300;

/// Make `text` safe to put in the notice: one line, no control, bidirectional, or zero-width
/// characters, and at most [`MAX_REASON_CHARS`] characters.
pub(crate) fn tidy_reason(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one_line = crate::util::strip_unsafe_chars(&spaced)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    crate::util::truncate_ellipsis(&one_line, MAX_REASON_CHARS)
}
