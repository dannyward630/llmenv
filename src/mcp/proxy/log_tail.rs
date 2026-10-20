//! The tail of the proxy log that an error shows.

use std::path::Path;

use super::{LOG_TAIL_BYTES, LOG_TAIL_LINES, log_path_for};

/// What [`tail_bounded_log`] found.
///
/// "Nothing to show" and "couldn't look" have to stay distinguishable: reporting
/// an unreadable log as though the proxy printed nothing tells the user the
/// opposite of the truth, and sends them to read a file they can't read.
pub(super) enum LogTail {
    Lines(String),
    Empty,
    Unreadable(std::io::Error),
}

/// Builds the trailing fragment of a startup-failure message: the last few lines
/// of the proxy log, or why they aren't available.
///
/// Replaces the previous message's guesses ("check that the port is free and
/// mcp-proxy is correctly installed"), which named two causes that were both
/// wrong in the incident that prompted #1086 — the real cause was an
/// `ImportError` visible only in the discarded stderr.
pub(super) fn proxy_log_hint(pid_path: &Path) -> String {
    let log_path = log_path_for(pid_path);
    match tail_bounded_log(&log_path, LOG_TAIL_LINES, LOG_TAIL_BYTES) {
        LogTail::Lines(tail) => format!("; last lines of {}:\n  {tail}", log_path.display()),
        LogTail::Empty => format!("; no output in {} either", log_path.display()),
        LogTail::Unreadable(e) => format!("; cannot read {} ({e})", log_path.display()),
    }
}

/// Reads up to `max_lines` trailing lines from `path`, scanning at most
/// `max_bytes` from the end.
///
/// Never fails — a diagnostic aid must not itself become the error — but does
/// report *why* it has nothing, so the caller can tell "the child was silent"
/// from "I couldn't open the log".
///
/// Decoded lossily: the child's stderr is arbitrary bytes, not guaranteed
/// UTF-8. Control characters are stripped, because these lines are
/// third-party output printed straight to the user's terminal and escape
/// sequences in them would be interpreted rather than shown.
pub(super) fn tail_bounded_log(path: &Path, max_lines: usize, max_bytes: u64) -> LogTail {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LogTail::Empty,
        Err(e) => return LogTail::Unreadable(e),
    };
    // A stat failure must not be treated as len 0: that would seek to the start
    // and quote the *first* bytes of the log under a "last lines" label.
    let len = match f.metadata() {
        Ok(m) => m.len(),
        Err(e) => return LogTail::Unreadable(e),
    };
    if let Err(e) = f.seek(SeekFrom::Start(len.saturating_sub(max_bytes))) {
        return LogTail::Unreadable(e);
    }
    let mut buf = Vec::new();
    // The bound matters beyond the seek: this is the child's live stderr, so
    // it can grow between the stat and the read.
    if let Err(e) = f.take(max_bytes).read_to_end(&mut buf) {
        return LogTail::Unreadable(e);
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .map(sanitize_log_line)
        .filter(|l| !l.trim().is_empty())
        .take(max_lines)
        .collect();
    if lines.is_empty() {
        return LogTail::Empty;
    }
    lines.reverse();
    LogTail::Lines(lines.join("\n  "))
}

/// Strips control characters from a log line before it is printed to a terminal.
///
/// The line is `mcp-proxy`'s stderr — third-party output that can echo request
/// data — and the caller writes it straight to the user's terminal, where escape
/// sequences would be acted on instead of displayed.
fn sanitize_log_line(line: &str) -> String {
    line.chars()
        .map(|c| {
            if c == '\t' || !c.is_control() {
                c
            } else {
                '\u{fffd}'
            }
        })
        .collect()
}
