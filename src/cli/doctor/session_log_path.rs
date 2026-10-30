//! Checks the session-log file path (#3138).
//!
//! The sink opens the file with `O_NOFOLLOW` and appends to it, so a symlink, a FIFO, or a
//! device at the path fails on every event. This check shows the problem once, with the fix.

use std::fs::FileType;
use std::io::ErrorKind;
use std::path::Path;

use crate::config::Config;

/// The result of the check on one resolved path.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// A regular file, or no file yet: the sink creates it on the first event.
    Usable { exists: bool },
    /// Something other than a regular file. The text names its kind.
    NotRegular(&'static str),
    /// The path cannot be read. The text holds the cause.
    Unreadable(String),
}

/// Print the session-log path check. Prints nothing when the file sink is off.
pub(super) fn run_doctor_session_log_path(use_color: bool, config: &Config) {
    let log = config.session_log_resolved();
    if !log.file.as_ref().is_some_and(|f| f.enabled) {
        return;
    }
    let pass = super::super::doctor_pass(use_color);
    let warn = super::super::doctor_warning(use_color);
    let path = match crate::session_log::resolve_file_path(log.file_path()) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("{warn} session log: cannot resolve the file path: {e:#}");
            return;
        }
    };
    let shown = path.display();
    match check_path(&path) {
        Verdict::Usable { exists: true } => eprintln!("{pass} session log file {shown}"),
        Verdict::Usable { exists: false } => {
            eprintln!("{pass} session log file {shown} (created on the first event)");
        }
        Verdict::NotRegular(kind) => eprintln!(
            "{warn} session log file {shown} is a {kind}, not a regular file, so no event is \
             written. Fix: remove the {kind}, or set session_log.file.path to a regular file."
        ),
        Verdict::Unreadable(cause) => eprintln!(
            "{warn} session log file {shown} cannot be read: {cause}. Fix: correct the \
             permissions, or set session_log.file.path to a writable location."
        ),
    }
}

/// Judge `path` without following a symlink, as the sink opens it.
fn check_path(path: &Path) -> Verdict {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Verdict::Usable { exists: true },
        Ok(meta) => Verdict::NotRegular(kind_name(meta.file_type())),
        Err(e) if e.kind() == ErrorKind::NotFound => Verdict::Usable { exists: false },
        Err(e) => Verdict::Unreadable(e.to_string()),
    }
}

fn kind_name(ft: FileType) -> &'static str {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if ft.is_fifo() {
            return "FIFO";
        }
        if ft.is_socket() {
            return "socket";
        }
        if ft.is_block_device() || ft.is_char_device() {
            return "device";
        }
    }
    if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "directory"
    } else {
        "special file"
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_regular_file_is_usable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, "").unwrap();
        assert_eq!(check_path(&path), Verdict::Usable { exists: true });
    }

    #[test]
    fn a_missing_file_is_usable() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            check_path(&dir.path().join("s.jsonl")),
            Verdict::Usable { exists: false }
        );
    }

    #[test]
    fn a_directory_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(check_path(dir.path()), Verdict::NotRegular("directory"));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_regular_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.jsonl");
        std::fs::write(&target, "").unwrap();
        let link = dir.path().join("s.jsonl");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(check_path(&link), Verdict::NotRegular("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let status = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(check_path(&path), Verdict::NotRegular("FIFO"));
    }

    #[cfg(unix)]
    #[test]
    fn a_socket_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert_eq!(check_path(&path), Verdict::NotRegular("socket"));
    }

    #[cfg(unix)]
    #[test]
    fn a_device_is_rejected() {
        assert_eq!(
            check_path(Path::new("/dev/null")),
            Verdict::NotRegular("device")
        );
    }
}
