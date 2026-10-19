//! The link policy of the `.llmenv.yaml` marker (#2946).
//!
//! A cloned repository controls the marker. A link to `/net/<host>/x` makes a stat or an open
//! contact that host, and a link out of the repository puts another file's `description` into
//! the agent's context. So the marker read follows a link only while each step stays inside the
//! folder that holds the marker. The walk runs `lstat` on one component at a time, inside that
//! folder only, so no path outside it is touched. A dotfile link such as
//! `~/.llmenv.yaml -> dotfiles/llmenv.yaml` stays inside `$HOME`, so it still works.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The most links that one marker read follows, as the kernel's own limit.
const MAX_LINKS: usize = 40;

/// Read the marker at `path` of at most `max_bytes` bytes, or `None` when there is none.
///
/// # Errors
/// The marker is a link whose target leaves its folder (the message names the marker and the
/// target), a link to a file that does not exist, or a file that the bounded read refuses.
pub(crate) fn read_marker(path: &Path, max_bytes: u64) -> io::Result<Option<String>> {
    let Some(dir) = path.parent() else {
        return Err(refused(path, path, "it has no parent folder"));
    };
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(at(path, e)),
        Ok(meta) if !meta.file_type().is_symlink() => {}
        Ok(_) => {
            let root = dir.canonicalize().map_err(|e| at(dir, e))?;
            let name = PathBuf::from(path.file_name().unwrap_or_default());
            let target = resolve_inside(&root, path, name)?;
            return match llmenv_paths::read_text_bounded(&target, max_bytes)
                .map_err(|e| at(&target, e))?
            {
                Some(text) => Ok(Some(text)),
                None => Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "it is a link to {}, which does not exist; correct or remove the link",
                        target.display()
                    ),
                )),
            };
        }
    }
    llmenv_paths::read_text_bounded(path, max_bytes)
}

/// The real path of `rel` under `root`, with every link resolved, or an error when a step leaves
/// `root`. `marker` names the marker in the error.
fn resolve_inside(root: &Path, marker: &Path, rel: PathBuf) -> io::Result<PathBuf> {
    let mut resolved = root.to_path_buf();
    let mut pending: VecDeque<OsString> = VecDeque::new();
    push_front(&mut pending, &rel, root, marker)?;
    let mut links = 0usize;
    while let Some(part) = pending.pop_front() {
        if part == ".." {
            if resolved == root {
                return Err(refused(
                    marker,
                    &resolved.join(".."),
                    "it leaves the folder",
                ));
            }
            resolved.pop();
            continue;
        }
        let next = resolved.join(&part);
        let is_link = match std::fs::symlink_metadata(&next) {
            Ok(meta) => meta.file_type().is_symlink(),
            // A missing step leaves the read to report it.
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(at(&next, e)),
        };
        if !is_link {
            resolved = next;
            continue;
        }
        links += 1;
        if links > MAX_LINKS {
            return Err(refused(marker, &next, "it follows too many links"));
        }
        let target = std::fs::read_link(&next).map_err(|e| at(&next, e))?;
        if target.is_absolute() {
            let Ok(inside) = target.strip_prefix(root) else {
                return Err(refused(marker, &target, "it leaves the folder"));
            };
            resolved = root.to_path_buf();
            push_front(&mut pending, inside, root, marker)?;
        } else {
            push_front(&mut pending, &target, root, marker)?;
        }
    }
    Ok(resolved)
}

/// `e` with the path of the step that failed, so the message names it.
fn at(path: &Path, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("{}: {e}", path.display()))
}

/// Put the components of `rel` in front of `pending`, in order.
fn push_front(
    pending: &mut VecDeque<OsString>,
    rel: &Path,
    root: &Path,
    marker: &Path,
) -> io::Result<()> {
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::ParentDir => parts.push(OsString::from("..")),
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                return Err(refused(marker, &root.join(rel), "it leaves the folder"));
            }
        }
    }
    for part in parts.into_iter().rev() {
        pending.push_front(part);
    }
    Ok(())
}

fn refused(marker: &Path, target: &Path, why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "the link {} points to {}, and {why}. llmenv follows a marker link only inside the \
             folder that holds the marker; replace the link with a regular file",
            marker.display(),
            target.display()
        ),
    )
}

#[cfg(all(test, unix))]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests;
