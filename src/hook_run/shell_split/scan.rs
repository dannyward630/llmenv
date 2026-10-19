//! The scan that finds the end of a `$(...)` substitution.

use std::iter::Peekable;
use std::str::Chars;

/// Take the text up to the `)` that closes a `$(` already read, counting nested parentheses.
/// Quotes and escapes group as they do in the outer splitter, so a `)` inside quotes is text.
pub(in crate::hook_run) fn take_until_close(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut depth = 0_usize;
    let (mut single, mut double) = (false, false);
    let mut inner = String::new();
    while let Some(c) = chars.next() {
        if single {
            single = c != '\'';
        } else {
            match c {
                '\\' => {
                    inner.push(c);
                    if let Some(next) = chars.next() {
                        inner.push(next);
                    }
                    continue;
                }
                '\'' if !double => single = true,
                '"' => double = !double,
                ')' if !double && depth == 0 => break,
                ')' if !double => depth = depth.saturating_sub(1),
                '(' if !double => depth = depth.saturating_add(1),
                _ => {}
            }
        }
        inner.push(c);
    }
    inner
}
