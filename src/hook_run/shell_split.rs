//! Shell command splitting for the task commit gate and the slippage check.
//!
//! The gate and the checks see a command as segments of words. A substitution is split out
//! first, so its command is a segment too. Split from `task_nudge` so the hook state code
//! stays separate from the parsing (#2627, #2542).

use std::iter::Peekable;
use std::str::Chars;

/// The deepest substitution nesting that the splitter follows. Deeper input fails closed, so
/// the hook process stack does not grow with the input (#2544).
pub(super) const MAX_SUBSTITUTION_DEPTH: usize = 32;

/// The command nests substitutions deeper than [`MAX_SUBSTITUTION_DEPTH`]. A caller treats the
/// command as unsafe.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NestingTooDeep;

/// The reason a deny gives for a command nested past [`MAX_SUBSTITUTION_DEPTH`]. It names the
/// limit and the fix, because a retry alone does not explain the refusal.
pub(super) fn nesting_reason() -> String {
    format!(
        "this command nests substitutions more than {MAX_SUBSTITUTION_DEPTH} levels deep, so \
         llmenv cannot check it. Split the command into smaller commands and run each one."
    )
}

/// Move each live `$(...)`, `<(...)`, `>(...)`, and backtick substitution to the start of the
/// segment that holds it, as its own `;`-separated part. Bash runs a substitution before the
/// command that holds it, so the verdict must follow that order (#2544). Single quotes keep
/// their text literal.
pub(super) fn split_substitutions(command: &str) -> Result<String, NestingTooDeep> {
    split_at_depth(command, 0)
}

/// [`split_substitutions`] for one nesting level. `depth` counts the substitutions around
/// `command`.
fn split_at_depth(command: &str, depth: usize) -> Result<String, NestingTooDeep> {
    if depth > MAX_SUBSTITUTION_DEPTH {
        return Err(NestingTooDeep);
    }
    let mut out = String::with_capacity(command.len());
    // The parts of the segment that is being read, and where that segment starts in `out`.
    let mut hoisted = String::new();
    let mut segment_start = 0;
    let mut chars = command.chars().peekable();
    let (mut single, mut double) = (false, false);
    while let Some(c) = chars.next() {
        let top = !single && !double;
        match c {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            // An escaped character is not a quote, and not a substitution start. This matches
            // `shell_segments`, so both passes agree on quote state.
            '\\' if !single => {
                out.push(c);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
                continue;
            }
            '$' if !single && chars.peek() == Some(&'(') => {
                chars.next();
                let inner = split_at_depth(&take_until_close(&mut chars), depth + 1)?;
                hoisted.push_str(&part(&inner));
                continue;
            }
            '`' if !single => {
                let inner: String = chars.by_ref().take_while(|&c| c != '`').collect();
                hoisted.push_str(&part(&split_at_depth(&inner, depth + 1)?));
                continue;
            }
            // Process substitution, `<(...)` and `>(...)`. Inside double quotes it is literal.
            // Bash runs it alongside its command, not before it, so it stays in place (#2544).
            '<' | '>' if !single && !double && chars.peek() == Some(&'(') => {
                chars.next();
                let inner = split_at_depth(&take_until_close(&mut chars), depth + 1)?;
                out.insert_str(segment_start, &hoisted);
                hoisted.clear();
                out.push_str(&part(&inner));
                segment_start = out.len();
                continue;
            }
            ';' | '|' | '&' | '\n' if top => {
                out.insert_str(segment_start, &hoisted);
                hoisted.clear();
                out.push(c);
                segment_start = out.len();
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    out.insert_str(segment_start, &hoisted);
    Ok(out)
}

/// One substitution as a `;`-terminated part. The `;` on each side is a segment boundary for
/// the splitter. The part sits outside any quote, because it goes at a segment start.
fn part(inner: &str) -> String {
    format!(";{inner};")
}

/// Take the text up to the `)` that closes a `$(` already read, counting nested parentheses.
/// Quotes and escapes group as they do in the outer splitter, so a `)` inside quotes is text.
pub(super) fn take_until_close(chars: &mut Peekable<Chars<'_>>) -> String {
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

/// Split `command` into segments at the shell operators `;`, `|`, `&`, and a newline, and each
/// segment into words. Quotes group a word and are dropped, so an operator inside quotes does not
/// split. Each `$(...)`, `<(...)`, `>(...)`, and backtick substitution is split out first, so its
/// command is a segment.
///
/// The arguments of an `eval` segment are split again, because `eval` runs them as a shell command.
/// Each inner command is added before the segment that holds it.
///
/// # Errors
/// Returns [`NestingTooDeep`] when the substitutions nest past the depth bound.
pub(super) fn shell_segments(command: &str) -> Result<Vec<Vec<String>>, NestingTooDeep> {
    let mut segments = Vec::new();
    for words in split_words(command)? {
        if let Some(inner) = eval_command(&words) {
            segments.extend(shell_segments(&inner)?);
        }
        segments.push(words);
    }
    Ok(segments)
}

/// The command that an `eval` segment runs: its arguments joined by spaces, or `None`.
/// The inner command is always shorter than the outer one, so the recursion ends.
fn eval_command(words: &[String]) -> Option<String> {
    match program_words(words).split_first() {
        Some((first, tail)) if program(first) == "eval" && !tail.is_empty() => Some(tail.join(" ")),
        _ => None,
    }
}

/// Split `command` into segments of words, without the `eval` expansion of [`shell_segments`].
fn split_words(command: &str) -> Result<Vec<Vec<String>>, NestingTooDeep> {
    let command = split_substitutions(command)?;
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars();
    let end_word = |words: &mut Vec<String>, word: &mut String, in_word: &mut bool| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                    in_word = true;
                }
            }
            (None, ';' | '|' | '&' | '\n') => {
                end_word(&mut words, &mut word, &mut in_word);
                if !words.is_empty() {
                    segments.push(std::mem::take(&mut words));
                }
            }
            (None, c) if c.is_whitespace() => end_word(&mut words, &mut word, &mut in_word),
            (None, c) => {
                word.push(c);
                in_word = true;
            }
        }
    }
    end_word(&mut words, &mut word, &mut in_word);
    if !words.is_empty() {
        segments.push(words);
    }
    Ok(segments)
}

/// The program name of a word: `/usr/bin/git` is `git`.
pub(super) fn program(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// A wrapper runs the command that follows its options. `value_opts` take a separate value word,
/// such as `-u root`. `operands` counts the plain words before the command, such as the duration
/// of `timeout`.
struct Wrapper {
    name: &'static str,
    value_opts: &'static [&'static str],
    operands: usize,
}

/// The wrappers that run a command with another environment, privilege, limit, or runner. Each
/// one is skipped with its options, so the word after them is the program (#2542).
const WRAPPERS: [Wrapper; 14] = [
    Wrapper {
        name: "env",
        value_opts: &["-u", "-C", "-S"],
        operands: 0,
    },
    Wrapper {
        name: "sudo",
        value_opts: &["-u", "-g", "-C", "-p", "-U"],
        operands: 0,
    },
    Wrapper {
        name: "doas",
        value_opts: &["-u", "-C"],
        operands: 0,
    },
    Wrapper {
        name: "nice",
        value_opts: &["-n"],
        operands: 0,
    },
    Wrapper {
        name: "ionice",
        value_opts: &["-c", "-n", "-p", "-P", "-u"],
        operands: 0,
    },
    Wrapper {
        name: "nohup",
        value_opts: &[],
        operands: 0,
    },
    Wrapper {
        name: "time",
        value_opts: &["-f", "-o"],
        operands: 0,
    },
    Wrapper {
        name: "exec",
        value_opts: &["-a"],
        operands: 0,
    },
    Wrapper {
        name: "command",
        value_opts: &[],
        operands: 0,
    },
    Wrapper {
        name: "timeout",
        value_opts: &["-s", "-k"],
        operands: 1,
    },
    Wrapper {
        name: "stdbuf",
        value_opts: &["-i", "-o", "-e"],
        operands: 0,
    },
    Wrapper {
        name: "setsid",
        value_opts: &[],
        operands: 0,
    },
    Wrapper {
        name: "xargs",
        value_opts: &["-a", "-d", "-E", "-I", "-L", "-n", "-P", "-s"],
        operands: 0,
    },
    Wrapper {
        name: "caffeinate",
        value_opts: &["-t", "-w"],
        operands: 0,
    },
];

/// The words of one segment after its group openers, env assignments, and wrappers with their
/// options. The first word is the program. The commit matcher and the task-end matcher both read a
/// segment this way.
pub(super) fn program_words(words: &[String]) -> Vec<&str> {
    let words: Vec<&str> = words
        .iter()
        .map(|w| w.trim_start_matches(['(', '{']))
        .filter(|w| !w.is_empty())
        .collect();
    let start = program_start(&words);
    words.get(start..).unwrap_or_default().to_vec()
}

/// The index of the program word: the first word that is not an env assignment, or a wrapper
/// with the words it takes.
fn program_start(words: &[&str]) -> usize {
    let mut index = 0;
    while let Some(word) = words.get(index) {
        if word.contains('=') && !word.starts_with('-') {
            index += 1;
        } else if let Some(wrapper) = WRAPPERS.iter().find(|w| w.name == program(word)) {
            index += 1 + wrapper_args(wrapper, words.get(index + 1..).unwrap_or_default());
        } else {
            break;
        }
    }
    index
}

/// The number of words that a wrapper takes after its name: options, the value of a value
/// option, env assignments, and operands. The count stops at the first word that is none of these.
fn wrapper_args(wrapper: &Wrapper, words: &[&str]) -> usize {
    let mut used = 0;
    let mut operands = wrapper.operands;
    while let Some(word) = words.get(used) {
        if word.starts_with('-') {
            used += 1 + usize::from(wrapper.value_opts.contains(word));
        } else if word.contains('=') || operands > 0 {
            operands = operands.saturating_sub(1);
            used += 1;
        } else {
            break;
        }
    }
    used.min(words.len())
}

/// Whether one segment starts a commit or a pull request.
fn starts_commit_or_pr(words: &[String]) -> bool {
    let words = program_words(words);
    let Some((first, tail)) = words.split_first() else {
        return false;
    };
    match program(first) {
        "gh" => {
            let mut iter = tail.iter();
            let mut subcommands: Vec<&str> = Vec::new();
            while let Some(word) = iter.next() {
                match *word {
                    "-R" | "--repo" => {
                        iter.next();
                    }
                    w if w.starts_with('-') => {}
                    w => subcommands.push(w),
                }
                if subcommands.len() == 2 {
                    break;
                }
            }
            subcommands == ["pr", "create"]
        }
        "git" => {
            let mut iter = tail.iter();
            while let Some(word) = iter.next() {
                match *word {
                    "-C" | "-c" | "--git-dir" | "--work-tree" => {
                        iter.next();
                    }
                    w if w.starts_with('-') => {}
                    w => return w == "commit",
                }
            }
            false
        }
        "sh" | "bash" | "zsh" => shell_script(tail).is_some_and(runs_commit_or_pr),
        _ => false,
    }
}

/// The script that a shell runs with `-c`. The flag may sit in a cluster, as in `bash -lc`.
/// An option that takes a value, such as `-o pipefail`, skips its value. A long option such as
/// `--noclobber` is not a cluster, and a script file has no `-c`.
pub(super) fn shell_script<'a>(args: &[&'a str]) -> Option<&'a str> {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let flags = arg.strip_prefix(['-', '+'])?;
        let short = !flags.starts_with('-');
        if arg.starts_with('-') && short && flags.contains('c') {
            return rest.next().copied();
        }
        let takes_value =
            matches!(*arg, "--rcfile" | "--init-file") || (short && flags.ends_with(['o', 'O']));
        if takes_value {
            rest.next();
        }
    }
    None
}

/// Whether the shell command runs `git commit` or `gh pr create` in any of its parts. A command
/// nested too deeply to split counts as one, so the gate fails closed.
pub(super) fn runs_commit_or_pr(command: &str) -> bool {
    match shell_segments(command) {
        Ok(segments) => segments.iter().any(|words| starts_commit_or_pr(words)),
        Err(NestingTooDeep) => true,
    }
}

/// Whether `llmenv task done <wip>` or `llmenv task wait <wip>` runs before the first commit or
/// pull request segment. Another task's end does not end the task `wip`. A command nested too
/// deeply to split may end the task, so it counts as ending it and the gate denies the commit.
pub(super) fn ends_task_before_commit(command: &str, wip: &str) -> bool {
    let Ok(segments) = shell_segments(command) else {
        return true;
    };
    for words in segments {
        if starts_commit_or_pr(&words) {
            return false;
        }
        if task_end_target(&words) == Some(wip) {
            return true;
        }
    }
    false
}

/// The task that a `llmenv task done` or `llmenv task wait` segment names, or `None`.
fn task_end_target(words: &[String]) -> Option<&str> {
    let words = program_words(words);
    match words.as_slice() {
        [first, "task", "done" | "wait", target, ..] if program(first) == "llmenv" => Some(*target),
        _ => None,
    }
}
