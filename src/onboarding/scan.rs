//! The bounded repository scan for onboarding (#2785). It gives the onboarding skill facts as
//! JSON and makes no judgment. Design: docs/design/issue-2773-onboarding.md (Q1).
//!
//! The scan reads file names and a fixed set of manifest keys. It never reads a source file, and
//! it never runs a script of a scanned repository. Everything it reports is data: the skill must
//! not follow an instruction found in it.

use std::collections::BTreeMap;
use std::os::fd::{AsFd as _, BorrowedFd};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::Serialize;

use crate::paths::dirfd;

mod manifest;
#[cfg(test)]
mod tests;

/// The deepest folder level below a root that the walk enters. Repositories usually sit one or
/// two levels below a folder such as `~/git`, so four levels leave room without a deep crawl.
const MAX_DEPTH: usize = 4;
/// The most repositories one scan reports. More than this stops the scan, because a plan with
/// hundreds of repositories is not one that a person can review.
const MAX_REPOSITORIES: usize = 200;
/// The most folders one scan reads. It bounds the time of a scan over a wide root.
const MAX_VISITED_DIRS: usize = 20_000;
/// The most names reported for one listing: the top level of a repository, its workflows, or its
/// scripts.
const MAX_NAMES: usize = 200;
/// The largest manifest that the scan reads. A real manifest is a few kilobytes.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// The most characters kept of one manifest value, such as a package name.
const MAX_VALUE_CHARS: usize = 200;

/// Folder names that the walk never enters. Hidden folders are skipped too.
const SKIPPED_NAMES: &[&str] = &["node_modules", "target", "vendor", "dist", "build"];

/// The bounds of one scan. Production uses [`Bounds::DEFAULT`]; a test can lower one bound.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    max_depth: usize,
    max_repositories: usize,
    max_visited_dirs: usize,
}

impl Bounds {
    const DEFAULT: Self = Self {
        max_depth: MAX_DEPTH,
        max_repositories: MAX_REPOSITORIES,
        max_visited_dirs: MAX_VISITED_DIRS,
    };
}

/// The result of one scan, printed as JSON.
#[derive(Debug, Serialize)]
struct ScanReport {
    roots: Vec<PathBuf>,
    limits: Limits,
    repositories: Vec<Repository>,
    skipped: Skipped,
}

/// The bounds that applied, so the reader can tell what the scan did not look at.
#[derive(Debug, Serialize)]
struct Limits {
    max_depth: usize,
    max_repositories: usize,
    max_visited_dirs: usize,
    max_names: usize,
    max_manifest_bytes: u64,
}

/// What the walk did not enter.
#[derive(Debug, Default, Serialize)]
struct Skipped {
    /// Folders not entered because they sit deeper than the depth bound.
    below_depth_bound: usize,
    /// Folders that could not be read, each with its error.
    unreadable: Vec<String>,
}

/// The evidence for one repository.
#[derive(Debug, Serialize)]
struct Repository {
    path: PathBuf,
    /// Top-level names. A folder name ends in `/`.
    names: Vec<String>,
    /// File names under `.github/workflows/`.
    workflows: Vec<String>,
    cargo: Option<CargoFacts>,
    package_json: Option<PackageJsonFacts>,
    pyproject: Option<PyprojectFacts>,
    /// Facts the scan could not read, such as a manifest that does not parse.
    notes: Vec<String>,
}

/// Keys from the `[package]` table of `Cargo.toml`, and whether it declares a workspace.
#[derive(Debug, Default, Serialize)]
struct CargoFacts {
    name: Option<String>,
    edition: Option<String>,
    rust_version: Option<String>,
    workspace: bool,
}

/// The script names of `package.json`, without the commands, and its package manager.
#[derive(Debug, Default, Serialize)]
struct PackageJsonFacts {
    scripts: Vec<String>,
    package_manager: Option<String>,
}

/// Keys of `pyproject.toml`: the project name, the Python bound, the build backend, and the
/// names of the `[tool.*]` tables.
#[derive(Debug, Default, Serialize)]
struct PyprojectFacts {
    name: Option<String>,
    requires_python: Option<String>,
    build_backend: Option<String>,
    tools: Vec<String>,
}

/// Scan `roots` and return the report as pretty JSON. The home folder and its ancestors are
/// refused as roots.
///
/// # Errors
/// `HOME` is not set or cannot be resolved, so the refusal cannot be checked. Otherwise the same
/// as [`scan`].
pub(crate) fn scan_json(roots: &[PathBuf]) -> anyhow::Result<String> {
    let home = crate::env_var::utf8_var("HOME")?.ok_or_else(|| {
        anyhow::anyhow!(
            "cannot scan: HOME is not set, so the scan cannot refuse the home folder. Fix: set HOME"
        )
    })?;
    let report = scan(roots, Path::new(&home), Bounds::DEFAULT)?;
    serde_json::to_string_pretty(&report).context("serializing the scan report")
}

/// Scan `roots` for repositories. `home` and every folder above it are refused as roots.
///
/// # Errors
/// `home` cannot be resolved, or a root is missing, is not a folder, cannot be read, or is `home`
/// or above it. The scan stops when it would pass the repository bound or the folder bound; the
/// error names the constant and the fix.
fn scan(roots: &[PathBuf], home: &Path, bounds: Bounds) -> anyhow::Result<ScanReport> {
    let home = std::fs::canonicalize(home).with_context(|| {
        format!(
            "cannot resolve the home folder {} to refuse it as a root. Fix: set HOME to a \
             folder that exists",
            home.display()
        )
    })?;
    let mut resolved = Vec::new();
    for root in roots {
        resolved.push(check_root(root, &home)?);
    }
    resolved.sort();
    resolved.dedup();
    let mut walk = Walk {
        bounds,
        visited: 0,
        found: BTreeMap::new(),
        skipped: Skipped::default(),
    };
    for root in &resolved {
        walk.root(root)?;
    }
    Ok(ScanReport {
        roots: resolved,
        limits: Limits {
            max_depth: bounds.max_depth,
            max_repositories: bounds.max_repositories,
            max_visited_dirs: bounds.max_visited_dirs,
            max_names: MAX_NAMES,
            max_manifest_bytes: MAX_MANIFEST_BYTES,
        },
        repositories: walk.found.into_values().collect(),
        skipped: walk.skipped,
    })
}

/// Resolve `root`, and refuse a root that is not a folder, or that is `home` or a folder above
/// it, such as `/` or `/Users`.
fn check_root(root: &Path, home: &Path) -> anyhow::Result<PathBuf> {
    let resolved = std::fs::canonicalize(root).with_context(|| {
        format!(
            "cannot scan {}: the folder cannot be resolved. Fix: pass a folder that exists",
            root.display()
        )
    })?;
    if !resolved.is_dir() {
        anyhow::bail!(
            "cannot scan {}: it is not a folder. Fix: pass a folder",
            root.display()
        );
    }
    if home.starts_with(&resolved) {
        anyhow::bail!(
            "cannot scan {}: the scan refuses the home folder and every folder above it, such \
             as `/`. Fix: pass a narrower folder, such as ~/git",
            root.display()
        );
    }
    Ok(resolved)
}

/// The state of one walk over all roots.
struct Walk {
    bounds: Bounds,
    visited: usize,
    /// Found repositories by path, so a repository under two roots is reported once.
    found: BTreeMap<PathBuf, Repository>,
    skipped: Skipped,
}

impl Walk {
    /// Walk one root. Each folder is opened relative to its parent without following a symlink,
    /// so the type check and the read name the same folder (src/paths/dirfd.rs).
    ///
    /// # Errors
    /// The root cannot be opened or read, or a bound is passed.
    fn root(&mut self, root: &Path) -> anyhow::Result<()> {
        let fd = dirfd::open_dir_nofollow(root).with_context(|| {
            format!(
                "cannot scan {}: the folder cannot be opened. Fix: grant read access, or pass \
                 another folder",
                root.display()
            )
        })?;
        match is_repository(fd.as_fd()) {
            Ok(true) => return self.found_repository(fd.as_fd(), root),
            Ok(false) => {}
            Err(e) => anyhow::bail!("cannot scan {}: {e}", root.display()),
        }
        let entries = self.read(fd.as_fd(), root)?.with_context(|| {
            format!(
                "cannot scan {}: the folder cannot be read. Fix: grant read access, or pass \
                 another folder",
                root.display()
            )
        })?;
        self.children(fd.as_fd(), root, &entries, 0)
    }

    /// Visit the folders of `dir`, which sits at `depth` below a root. The recursion is at most
    /// `max_depth` deep, so the walk holds at most that many open folders.
    fn children(
        &mut self,
        dir: BorrowedFd<'_>,
        path: &Path,
        entries: &[dirfd::Entry],
        depth: usize,
    ) -> anyhow::Result<()> {
        let child_depth = depth.saturating_add(1);
        for entry in entries.iter().filter(|e| e.is_dir() && e.name != ".git") {
            let child_path = path.join(&entry.name);
            if child_depth > self.bounds.max_depth {
                self.skipped.below_depth_bound = self.skipped.below_depth_bound.saturating_add(1);
                continue;
            }
            let child = match dirfd::open_dir_at(dir, &entry.name) {
                Ok(child) => child,
                Err(e) => {
                    self.unreadable(&child_path, &e);
                    continue;
                }
            };
            match is_repository(child.as_fd()) {
                Ok(true) => self.found_repository(child.as_fd(), &child_path)?,
                Ok(false) if enters(&entry.name.to_string_lossy()) => {
                    if let Ok(grandchildren) = self.read(child.as_fd(), &child_path)? {
                        self.children(child.as_fd(), &child_path, &grandchildren, child_depth)?;
                    }
                }
                Ok(false) => {}
                Err(e) => self.unreadable(&child_path, &e),
            }
        }
        Ok(())
    }

    /// Read the entries of an open folder and count it against the folder bound. A read error
    /// is recorded and returned as the inner `Err`; passing the bound is the outer one.
    fn read(
        &mut self,
        dir: BorrowedFd<'_>,
        path: &Path,
    ) -> anyhow::Result<std::io::Result<Vec<dirfd::Entry>>> {
        self.visited = self.visited.saturating_add(1);
        if self.visited > self.bounds.max_visited_dirs {
            anyhow::bail!(
                "the scan read more than MAX_VISITED_DIRS ({}) folders. Fix: pass a narrower root",
                self.bounds.max_visited_dirs
            );
        }
        let entries = dirfd::read_dir_entries(dir);
        if let Err(e) = &entries {
            self.unreadable(path, e);
        }
        Ok(entries)
    }

    fn unreadable(&mut self, path: &Path, error: &std::io::Error) {
        self.skipped
            .unreadable
            .push(format!("{}: {error}", path.display()));
    }

    fn found_repository(&mut self, dir: BorrowedFd<'_>, path: &Path) -> anyhow::Result<()> {
        if self.found.contains_key(path) {
            return Ok(());
        }
        if self.found.len() >= self.bounds.max_repositories {
            anyhow::bail!(
                "the scan found more than MAX_REPOSITORIES ({}) repositories. Fix: pass a \
                 narrower root",
                self.bounds.max_repositories
            );
        }
        self.found
            .insert(path.to_path_buf(), manifest::describe(dir, path));
        Ok(())
    }
}

/// Whether the walk enters a folder with this name.
fn enters(name: &str) -> bool {
    !name.starts_with('.') && !SKIPPED_NAMES.contains(&name)
}

/// A repository has a `.git` folder, or a `.git` file for a worktree or a submodule. A `.git`
/// symlink does not count, because the scan does not follow links.
fn is_repository(dir: BorrowedFd<'_>) -> std::io::Result<bool> {
    let Some(st) = dirfd::stat_at_nofollow(dir, std::ffi::OsStr::new(".git"))? else {
        return Ok(false);
    };
    let kind = rustix::fs::FileType::from_raw_mode(st.st_mode as rustix::fs::RawMode);
    Ok(matches!(
        kind,
        rustix::fs::FileType::Directory | rustix::fs::FileType::RegularFile
    ))
}
