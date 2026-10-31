//! Copies one state file between the durable store and a config folder (#3139).

use std::path::Path;

/// Copy the regular file `src` to `dst`. Does nothing when `dst` already exists.
///
/// The source opens with a link refusal. The destination opens with `create_new`, which refuses an
/// existing path and a dangling link. Both checks run on the open, so a link swapped in after an
/// earlier check cannot redirect the copy. The copy keeps the source mode, as `std::fs::copy` does.
pub(super) fn copy_file_no_follow(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut src_opts = std::fs::OpenOptions::new();
    src_opts.read(true);
    let mut input = crate::paths::refuse_symlink(&mut src_opts).open(src)?;
    let mode = input.metadata()?.permissions();
    let mut dst_opts = std::fs::OpenOptions::new();
    dst_opts.write(true).create_new(true);
    let mut output = match dst_opts.open(dst) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(e) => return Err(e),
    };
    std::io::copy(&mut input, &mut output)?;
    output.set_permissions(mode)
}

/// True when `path` is a regular file (`symlink_metadata`, not `is_file()`
/// — #1341: `is_file()` follows a symlink, so a symlinked entry under
/// `state_dir`/`config_dir` would otherwise be read/copied through
/// silently). Skips (warns) rather than errors: a stray symlink here
/// shouldn't block a whole `materialize`/inherit pass over one file. A
/// missing source is the documented, common no-op case (nothing cached
/// yet) and stays silent; any other stat failure (permission denied, ...)
/// is warned rather than folded into the same "absent" outcome
/// (security-audit, #1341).
pub(super) fn is_real_file(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            // The skip is intended (#1341), so it runs on every pass and is debug only.
            tracing::debug!(path = %path.display(), "inherit: skipping symlinked file");
            false
        }
        Ok(meta) => meta.is_file(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "inherit: could not stat file, skipping");
            false
        }
    }
}

/// True when something already exists at `path`, including a dangling
/// symlink (`symlink_metadata`, not `exists()` — #1341 security-audit:
/// `exists()` follows a symlink and reports `false` for a dangling one,
/// which would fall through to a copy opening
/// `O_CREAT|O_TRUNC` through the link and writing the copied content at
/// the link's target instead of replacing the link itself).
pub(super) fn dest_already_present(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                tracing::warn!(
                    path = %path.display(),
                    "inherit: skipping copy — destination is a symlink"
                );
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        // The destination state is unknown, so the copy is skipped. A copy here could write
        // through a link the check could not see (#1341), so the skip is the safe result.
        Err(e) => {
            tracing::error!(
                path = %path.display(),
                error = %e,
                "inherit: skipping copy — cannot read the destination. Fix: check the \
                 destination's permissions, then run the command again."
            );
            true
        }
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::expect_used, reason = "a test: a failed setup must stop it")]
mod tests {
    use super::*;

    #[test]
    fn a_link_at_the_destination_is_not_written_through() {
        let dir = tempfile::tempdir().expect("temp dir");
        let src = dir.path().join("src.jsonl");
        std::fs::write(&src, b"new\n").expect("write the source");
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, b"keep me").expect("write the victim");
        let dst = dir.path().join("dst.jsonl");
        std::os::unix::fs::symlink(&victim, &dst).expect("plant a link at the destination");
        copy_file_no_follow(&src, &dst).expect("an existing destination is a skip");
        assert_eq!(std::fs::read(&victim).expect("read the victim"), b"keep me");
    }

    #[test]
    fn a_link_as_the_source_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let real = dir.path().join("real.jsonl");
        std::fs::write(&real, b"secret\n").expect("write the real file");
        let src = dir.path().join("src.jsonl");
        std::os::unix::fs::symlink(&real, &src).expect("plant a link as the source");
        let dst = dir.path().join("dst.jsonl");
        assert!(
            copy_file_no_follow(&src, &dst).is_err(),
            "a linked source must be refused"
        );
        assert!(!dst.exists(), "nothing may be copied from a link");
    }

    #[test]
    fn a_regular_file_is_copied_with_its_content() {
        let dir = tempfile::tempdir().expect("temp dir");
        let src = dir.path().join("src.jsonl");
        std::fs::write(&src, b"line\n").expect("write the source");
        let dst = dir.path().join("dst.jsonl");
        copy_file_no_follow(&src, &dst).expect("copy a regular file");
        assert_eq!(std::fs::read(&dst).expect("read the copy"), b"line\n");
    }
}
