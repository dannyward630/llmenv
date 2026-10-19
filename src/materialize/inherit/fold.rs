//! The fold of a stranded transcript folder into the durable store, by file descriptor (#1066).

use std::path::Path;

use anyhow::Context as _;

/// Move `src` into `dst` recursively, keeping whichever file is newer.
///
/// Renames rather than copies: the stranded folders and the durable store both
/// live under the cache dir, so this is a metadata operation instead of a bulk
/// copy. That matters — a real machine had 1 GiB of transcripts stranded across
/// 27 folders, and this runs from `export`, which fires on every shell prompt.
/// Falls back to copy-then-delete when `rename` can't be used (cross-device).
///
/// Symlinked *entries* are skipped rather than followed — there's no TOCTOU-safe
/// way to follow one into a bounded tree, matching the discipline in
/// `adapter::claude_code::copy_dir_owner_only`.
///
/// That guarantee is per-entry, not whole-tree (#1066). `src` itself is a path
/// the kernel re-resolves on every syscall, so swapping it for a symlink between
/// the check and the `read_dir` would still redirect the walk. Closing that
/// needs `openat`-relative traversal — opening the parent once and working from
/// its file descriptor — applied across every tree walk in the repo, which is a
/// dependency decision (`cap-std`/`rustix`) plus a cross-cutting refactor rather
/// than a fix to this function. Severity is low: it requires winning a race as
/// the same user, who can already read these files.
pub(super) fn move_dir_newest_wins(src: &Path, dst: &Path) -> anyhow::Result<()> {
    crate::adapter::skills::create_dir_owner_only(dst)
        .with_context(|| format!("creating {}", dst.display()))?;
    // `create_dir_owner_only`/`create_dir_all` is a no-op (success) when `dst`
    // already resolves through a symlink to an existing directory — it never
    // lstats first. Without this check every following `rename`/`copy` in
    // this call would land inside the symlink's target instead of the
    // intended tree (#1065).
    anyhow::ensure!(
        std::fs::symlink_metadata(dst).is_ok_and(|md| md.file_type().is_dir()),
        "durable transcript dir {} is not a real directory",
        dst.display()
    );
    // #1066: both ends are opened once and every entry below is reached
    // through those descriptors. `src`/`dst` continue downward for error
    // messages only — the kernel never re-resolves them, so a directory
    // swapped for a symlink after the checks above cannot redirect the move.
    use std::os::fd::AsFd as _;
    let src_dir = crate::paths::dirfd::open_dir_nofollow(src)
        .with_context(|| format!("reading {}", src.display()))?;
    let dst_dir = crate::paths::dirfd::open_dir_nofollow(dst)
        .with_context(|| format!("opening {}", dst.display()))?;
    let entries = crate::paths::dirfd::read_dir_entries(src_dir.as_fd())
        .with_context(|| format!("reading entry in {}", src.display()))?;
    for entry in entries {
        let from = src.join(&entry.name);
        let to = dst.join(&entry.name);
        if entry.is_symlink() {
            tracing::debug!("inherit: skipping symlink {}", from.display());
        } else if entry.is_dir() {
            move_dir_newest_wins(&from, &to)?;
        } else if entry.is_file() && is_newer_at(src_dir.as_fd(), dst_dir.as_fd(), &entry.name) {
            move_file_at(src_dir.as_fd(), dst_dir.as_fd(), &entry.name, &from, &to)?;
        }
    }
    Ok(())
}

/// Rename `from` onto `to`, falling back to copy-then-delete across devices.
/// Move `name` from one open directory to another, newest-wins already decided.
///
/// `from`/`to` are for error messages only. `renameat` is tried first and the
/// copy fallback exists for the cross-filesystem (`EXDEV`) case, which is why
/// the durable store being on a different mount doesn't break inheritance.
fn move_file_at(
    src_dir: std::os::fd::BorrowedFd<'_>,
    dst_dir: std::os::fd::BorrowedFd<'_>,
    name: &std::ffi::OsStr,
    from: &Path,
    to: &Path,
) -> anyhow::Result<()> {
    use crate::paths::dirfd;

    if dirfd::rename_at(src_dir, name, dst_dir, name).is_ok() {
        return Ok(());
    }
    // Owner-only, matching everything else llmenv writes into the durable
    // store; the replace semantics (unlink, then create) are what keep a
    // symlink at the destination from being written through (#1065). The copy
    // streams, because a transcript can be hundreds of MiB.
    dirfd::copy_file_at(src_dir, dst_dir, name, rustix::fs::Mode::from(0o600))
        .with_context(|| format!("copying {} -> {}", from.display(), to.display()))?;
    if let Err(e) = dirfd::remove_file_at(src_dir, name) {
        tracing::warn!(
            "inherit: copied {} to {}, but could not remove the source, so both copies exist: {e}",
            from.display(),
            to.display()
        );
    }
    Ok(())
}

/// Whether `name` in `src_dir` is newer than the same name in `dst_dir`.
/// A destination that can't be read counts as older, so the source wins. A source that can't
/// be read is not migrated. Each read error is logged, so the skip is visible.
fn is_newer_at(
    src_dir: std::os::fd::BorrowedFd<'_>,
    dst_dir: std::os::fd::BorrowedFd<'_>,
    name: &std::ffi::OsStr,
) -> bool {
    let src = match crate::paths::dirfd::mtime_at(src_dir, name) {
        Ok(Some(src)) => src,
        Ok(None) => return false,
        Err(e) => {
            tracing::error!(
                "inherit: cannot read the age of {}, so it is not migrated: {e}",
                name.display()
            );
            return false;
        }
    };
    match crate::paths::dirfd::mtime_at(dst_dir, name) {
        Ok(Some(dst)) => src > dst,
        Ok(None) => true,
        Err(e) => {
            tracing::error!(
                "inherit: cannot read the age of the copy of {}, so the source wins: {e}",
                name.display()
            );
            true
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    // #1065: `copy_replacing`'s whole contract is "never write through a
    // symlink at `to`" — a plain `std::fs::copy` would follow the symlink
    // and overwrite whatever it points at instead of replacing the symlink
    // itself.
    //
    // #1066: both properties now belong to the fd-relative helpers — the
    // path-based `copy_replacing`/`is_newer` they were written against are
    // gone, replaced by `copy_file_at`/`is_newer_at`, so the tests move with
    // them rather than being deleted alongside the code.
    #[cfg(unix)]
    #[test]
    fn moving_a_file_replaces_a_symlink_at_the_destination() {
        use std::os::fd::AsFd as _;

        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        write(&src.join("f"), "new-content");
        let victim = tmp.path().join("victim");
        write(&victim, "must-not-be-touched");
        std::os::unix::fs::symlink(&victim, dst.join("f")).unwrap();

        let src_dir = crate::paths::dirfd::open_dir_nofollow(&src).unwrap();
        let dst_dir = crate::paths::dirfd::open_dir_nofollow(&dst).unwrap();
        move_file_at(
            src_dir.as_fd(),
            dst_dir.as_fd(),
            std::ffi::OsStr::new("f"),
            &src.join("f"),
            &dst.join("f"),
        )
        .unwrap();

        assert!(
            !std::fs::symlink_metadata(dst.join("f"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink at the destination must be replaced, not written through"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("f")).unwrap(),
            "new-content"
        );
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "must-not-be-touched",
            "the symlink's target must be untouched"
        );
    }

    // #1065: following a symlink at the destination for the mtime comparison
    // would read the *target's* mtime — an old target makes a planted symlink
    // look "safe to clobber" and selects the vulnerable move path.
    #[cfg(unix)]
    #[test]
    fn is_newer_at_does_not_follow_a_symlink_at_destination() {
        use std::os::fd::AsFd as _;

        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        write(&src.join("f"), "content");
        let old_target = tmp.path().join("old-target");
        write(&old_target, "old");
        filetime::set_file_mtime(&old_target, filetime::FileTime::from_unix_time(1, 0)).unwrap();
        std::os::unix::fs::symlink(&old_target, dst.join("f")).unwrap();

        let src_dir = crate::paths::dirfd::open_dir_nofollow(&src).unwrap();
        let dst_dir = crate::paths::dirfd::open_dir_nofollow(&dst).unwrap();
        assert!(
            !is_newer_at(src_dir.as_fd(), dst_dir.as_fd(), std::ffi::OsStr::new("f")),
            "a symlink at the destination is compared by its own metadata, not its target's"
        );
    }

    // #1065: `create_dir_owner_only`/`create_dir_all` succeeds (no-op) when
    // `dst` already resolves through a symlink to an existing directory —
    // it never lstats first. `move_dir_newest_wins` must refuse to proceed
    // rather than silently folding into the symlink's target.
    #[cfg(unix)]
    #[test]
    fn move_dir_newest_wins_rejects_a_symlinked_destination() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        write(&src.join("a.jsonl"), "content");
        let real_other_dir = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&real_other_dir).unwrap();
        let dst = tmp.path().join("dst-symlink");
        std::os::unix::fs::symlink(&real_other_dir, &dst).unwrap();

        let err = move_dir_newest_wins(&src, &dst).expect_err(
            "a destination that resolves through a symlink must be rejected, not folded into",
        );
        assert!(err.to_string().contains("not a real directory"));
        assert!(
            std::fs::read_dir(&real_other_dir).unwrap().next().is_none(),
            "nothing must have been written into the symlink's target"
        );
    }
}
