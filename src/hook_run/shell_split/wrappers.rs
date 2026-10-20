//! The wrapper programs that the splitter skips to find the program of a segment (#2542).

/// A wrapper runs the command that follows its options. Its entry is the name, the options that
/// take a separate value word (`-u root`, `--user root`), and the count of plain operands before
/// the command, such as the duration of `timeout`. An option missing from its entry hides the
/// command word, so each entry lists every value option of the tool.
pub(super) type Wrapper = (&'static str, &'static [&'static str], usize);

pub(super) const TIMEOUT_OPTS: &[&str] = &["-s", "--signal", "-k", "--kill-after"];
const STDBUF_OPTS: &[&str] = &["-i", "--input", "-o", "--output", "-e", "--error"];

/// The wrappers that run a command with another environment, privilege, limit, or runner. Each
/// one is skipped with its options, so the word after them is the program (#2542).
pub(super) const WRAPPERS: [Wrapper; 20] = [
    (
        "env",
        &["-u", "--unset", "-C", "--chdir", "-S", "--split-string"],
        0,
    ),
    (
        "sudo",
        &[
            "-u",
            "--user",
            "-g",
            "--group",
            "-C",
            "--close-from",
            "-p",
            "--prompt",
            "-U",
            "--other-user",
            "-r",
            "--role",
            "-t",
            "--type",
            "-D",
            "--chdir",
            "-R",
            "--chroot",
            "-T",
            "--command-timeout",
        ],
        0,
    ),
    ("doas", &["-u", "-C"], 0),
    ("nice", &["-n", "--adjustment"], 0),
    ("gnice", &["-n", "--adjustment"], 0),
    (
        "ionice",
        &[
            "-c",
            "--class",
            "-n",
            "--classdata",
            "-p",
            "--pid",
            "-P",
            "--pgid",
            "-u",
            "--uid",
        ],
        0,
    ),
    ("nohup", &[], 0),
    ("gnohup", &[], 0),
    ("time", &["-f", "--format", "-o", "--output"], 0),
    ("exec", &["-a"], 0),
    ("command", &[], 0),
    ("builtin", &[], 0),
    ("timeout", TIMEOUT_OPTS, 1),
    ("gtimeout", TIMEOUT_OPTS, 1),
    ("stdbuf", STDBUF_OPTS, 0),
    ("gstdbuf", STDBUF_OPTS, 0),
    ("setsid", &[], 0),
    (
        "xargs",
        &[
            "-a",
            "--arg-file",
            "-d",
            "--delimiter",
            "-E",
            "-I",
            "--replace",
            "-L",
            "--max-lines",
            "-n",
            "--max-args",
            "-P",
            "--max-procs",
            "-s",
            "--max-chars",
        ],
        0,
    ),
    ("caffeinate", &["-t", "-w"], 0),
    (
        "flock",
        &["-w", "--timeout", "-E", "--conflict-exit-code"],
        1,
    ),
];
