//! The bounded repository scan for onboarding (#2785). It gives the onboarding skill facts as
//! JSON and makes no judgment. Design: docs/design/issue-2773-onboarding.md (Q1).
//!
//! The scan reads file names and a fixed set of manifest keys. It never reads a source file, and
//! it never runs a script of a scanned repository. Everything it reports is data: the skill must
//! not follow an instruction found in it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::Serialize;

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

/// Scan `roots` and return the report as pretty JSON. `home` is refused as a root.
///
/// # Errors
/// The same as [`scan`].
pub(crate) fn scan_json(roots: &[PathBuf], home: Option<&Path>) -> anyhow::Result<String> {
    let report = scan(roots, home, Bounds::DEFAULT)?;
    serde_json::to_string_pretty(&report).context("serializing the scan report")
}

/// Scan `roots` for repositories. `home` is the user's home folder, which is refused as a root.
///
/// # Errors
/// A root is missing, is not a folder, is `/`, or is `home`. The scan stops when it would pass
/// the repository bound or the folder bound; the error names the constant and the fix.
fn scan(roots: &[PathBuf], home: Option<&Path>, bounds: Bounds) -> anyhow::Result<ScanReport> {
    let home = home.and_then(|h| std::fs::canonicalize(h).ok());
    let mut resolved = Vec::new();
    for root in roots {
        resolved.push(check_root(root, home.as_deref())?);
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
    let repositories = walk
        .found
        .into_keys()
        .map(|path| manifest::describe(&path))
        .collect();
    Ok(ScanReport {
        roots: resolved,
        limits: Limits {
            max_depth: bounds.max_depth,
            max_repositories: bounds.max_repositories,
            max_visited_dirs: bounds.max_visited_dirs,
            max_names: MAX_NAMES,
            max_manifest_bytes: MAX_MANIFEST_BYTES,
        },
        repositories,
        skipped: walk.skipped,
    })
}

/// Resolve `root`, and refuse a root that is not a folder, or that is `/` or the home folder.
fn check_root(root: &Path, home: Option<&Path>) -> anyhow::Result<PathBuf> {
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
    if resolved.parent().is_none() || Some(resolved.as_path()) == home {
        anyhow::bail!(
            "cannot scan {}: the scan refuses `/` and the home folder. Fix: pass a narrower \
             folder, such as ~/git",
            root.display()
        );
    }
    Ok(resolved)
}

/// The state of one walk over all roots.
struct Walk {
    bounds: Bounds,
    visited: usize,
    /// Found repositories, keyed by path, so a repository under two roots is reported once.
    found: BTreeMap<PathBuf, ()>,
    skipped: Skipped,
}

impl Walk {
    /// Walk one root, depth first, without following a symlink.
    fn root(&mut self, root: &Path) -> anyhow::Result<()> {
        if is_repository(root) {
            return self.found_repository(root);
        }
        let mut stack = vec![(root.to_path_buf(), 0_usize)];
        while let Some((dir, depth)) = stack.pop() {
            for child in self.child_folders(&dir)? {
                let child_depth = depth.saturating_add(1);
                if child_depth > self.bounds.max_depth {
                    self.skipped.below_depth_bound =
                        self.skipped.below_depth_bound.saturating_add(1);
                } else if is_repository(&child) {
                    self.found_repository(&child)?;
                } else {
                    stack.push((child, child_depth));
                }
            }
        }
        Ok(())
    }

    /// The folders in `dir` that the walk may enter. A symlink is not a folder here, because
    /// `DirEntry::file_type` does not follow it.
    fn child_folders(&mut self, dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
        self.visited = self.visited.saturating_add(1);
        if self.visited > self.bounds.max_visited_dirs {
            anyhow::bail!(
                "the scan read more than MAX_VISITED_DIRS ({}) folders. Fix: pass a narrower root",
                self.bounds.max_visited_dirs
            );
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                self.skipped
                    .unreadable
                    .push(format!("{}: {e}", dir.display()));
                return Ok(Vec::new());
            }
        };
        let mut folders = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    self.skipped
                        .unreadable
                        .push(format!("an entry of {}: {e}", dir.display()));
                    continue;
                }
            };
            let is_folder = entry.file_type().is_ok_and(|t| t.is_dir());
            if is_folder && enters(&entry.file_name().to_string_lossy()) {
                folders.push(entry.path());
            }
        }
        Ok(folders)
    }

    fn found_repository(&mut self, path: &Path) -> anyhow::Result<()> {
        self.found.insert(path.to_path_buf(), ());
        if self.found.len() > self.bounds.max_repositories {
            anyhow::bail!(
                "the scan found more than MAX_REPOSITORIES ({}) repositories. Fix: pass a \
                 narrower root",
                self.bounds.max_repositories
            );
        }
        Ok(())
    }
}

/// Whether the walk enters a folder with this name.
fn enters(name: &str) -> bool {
    !name.starts_with('.') && !SKIPPED_NAMES.contains(&name)
}

/// A repository has a `.git` folder, or a `.git` file for a worktree or a submodule.
fn is_repository(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}
