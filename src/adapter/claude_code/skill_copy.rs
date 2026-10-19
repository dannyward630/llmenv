//! The owner-only copy of a skill folder, which walks by file descriptor (#1066).

use std::path::{Path, PathBuf};

use anyhow::Context as _;

use crate::adapter::skills::create_dir_owner_only;

/// The largest file that a skill folder copies. The largest skill asset in common use is a few
/// MiB, so a file this large is a mistake, and the copy holds the whole file in memory (#2942).
const MAX_SKILL_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Copy files from a source directory into a destination recursively, writing
/// each file owner-only (0o600). Non-UTF-8 paths are skipped (same policy as
/// `scan_skill_files_for_hardcoded_paths`). Returns the list of relative paths
/// written (relative to `dest_dir`), for inclusion in the `owned` set.
pub(crate) fn copy_dir_owner_only(src: &Path, dest: &Path) -> anyhow::Result<Vec<PathBuf>> {
    // #1066: the walk descends by file descriptor, not by path. `src` is
    // resolved exactly once, here; every step below opens an entry of a
    // directory this process already holds open, so there is no window in
    // which a directory can be swapped for a symlink between the check and the
    // use. The `src` paths further down are carried for error messages only
    // and are never handed back to the kernel.
    use std::os::fd::AsFd as _;

    let dir = crate::paths::dirfd::open_dir_nofollow(src)
        .with_context(|| format!("opening source directory '{}'", src.display()))?;
    copy_dir_owner_only_inner(dir.as_fd(), src, dest, true)
}

fn copy_dir_owner_only_inner(
    src_dir: std::os::fd::BorrowedFd<'_>,
    src: &Path,
    dest: &Path,
    is_root: bool,
) -> anyhow::Result<Vec<PathBuf>> {
    // #1341: a symlink already at `dest` (planted, or left behind by a prior
    // run) is never legitimate — llmenv owns everything under the
    // materialized output tree. Reject before create_dir_owner_only, which
    // would otherwise follow it (chmod-ing and writing through the target).
    // A stat error other than "not found" fails closed (propagated) rather
    // than being treated as "nothing there, proceed" — the checked path
    // being currently unreadable is not the same as it being absent
    // (security-audit, #1341).
    match std::fs::symlink_metadata(dest) {
        Ok(meta) if meta.file_type().is_symlink() => {
            anyhow::bail!(
                "destination path '{}' is a symlink — a path llmenv owns and writes through \
                 must never be a symlink",
                dest.display()
            );
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e)
                .with_context(|| format!("checking destination path '{}'", dest.display()));
        }
    }
    use std::os::fd::AsFd as _;

    let mut written: Vec<PathBuf> = Vec::new();
    create_dir_owner_only(dest)?;
    for entry in crate::paths::dirfd::read_dir_entries(src_dir)? {
        let file_name = entry.name.clone();
        let src_path = src.join(&file_name);
        if entry.is_symlink() {
            // #1341: a symlinked SKILL.md at the *skill's own root* is
            // silently dropped by the skip below, producing a skill
            // directory that only fails validation later with a misleading
            // "missing SKILL.md" — the file *is* there in the source, it
            // just never got copied. Fail loud here instead, where the real
            // cause is still in scope. Root-only and case-insensitive
            // (security-audit, #1341): a nested `SKILL.md` (a vendored
            // sub-skill, an example file) isn't the manifest
            // `validate_skills` checks, and macOS's default
            // case-insensitive filesystem would otherwise let `skill.md`
            // take the silent-skip branch and still satisfy
            // `skill_md.exists()`.
            if is_root && file_name.eq_ignore_ascii_case("SKILL.md") {
                anyhow::bail!(
                    "'{}' is a symlink — a skill's SKILL.md must be a real file, not a \
                     symlink",
                    src_path.display()
                );
            }
            // Any other symlinked entry (reference file, helper script) is
            // skipped, not fatal — no TOCTOU-safe way to follow it into a
            // bounded dir. The skip prints to stderr (#1341): `tracing` warnings are off at
            // the default log level, and the skill still validates as if the reference
            // existed.
            eprintln!(
                "warning: skipping the symlink {} inside a skill; a skill folder copies real \
                 files only",
                src_path.display()
            );
            continue;
        }
        let dest_path = dest.join(&file_name);
        if entry.is_dir() {
            let child = crate::paths::dirfd::open_dir_at(src_dir, &file_name)
                .with_context(|| format!("opening source directory '{}'", src_path.display()))?;
            let sub_written =
                copy_dir_owner_only_inner(child.as_fd(), &src_path, &dest_path, false)?;
            written.extend(sub_written);
        } else if entry.is_file() {
            // #1341: a symlink already at `dest_path` is never legitimate —
            // `write_owner_only` opens with `create(true).truncate(true)`,
            // which follows a symlink and overwrites its target.
            if let Ok(dest_meta) = std::fs::symlink_metadata(&dest_path)
                && dest_meta.file_type().is_symlink()
            {
                anyhow::bail!(
                    "destination path '{}' is a symlink — a path llmenv owns and writes \
                     through must never be a symlink",
                    dest_path.display()
                );
            }
            let content =
                crate::paths::dirfd::read_file_at(src_dir, &file_name, MAX_SKILL_FILE_BYTES)
                    .with_context(|| format!("reading '{}'", src_path.display()))?;
            crate::paths::write_owner_only(&dest_path, &content)?;
            written.push(dest_path);
        } else {
            eprintln!(
                "warning: skipping {} inside a skill; it is not a regular file or a folder",
                src_path.display()
            );
        }
    }
    Ok(written)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn a_fifo_is_skipped_and_a_file_over_the_cap_fails_with_its_name() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("SKILL.md"), "---\nname: s\n---\n").unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(src.path().join("pipe"))
            .status()
            .unwrap();
        assert!(made.success());
        let written = copy_dir_owner_only(src.path(), &out.path().join("a")).unwrap();
        assert_eq!(written, vec![out.path().join("a/SKILL.md")]);

        let big = src.path().join("big.bin");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_SKILL_FILE_BYTES + 1).unwrap();
        let err = format!(
            "{:#}",
            copy_dir_owner_only(src.path(), &out.path().join("b")).unwrap_err()
        );
        assert!(err.contains(&big.display().to_string()), "{err}");
        assert!(err.contains("more than"), "{err}");
    }
}
