//! The facts that the scan reads from one repository: top-level names, workflow names, and a
//! fixed set of manifest keys (#2785). Every read is relative to the open repository folder and
//! does not follow a symlink.

use std::ffi::OsStr;
use std::os::fd::{AsFd as _, BorrowedFd};
use std::path::Path;

use rustix::io::Errno;

use super::{
    CargoFacts, MAX_MANIFEST_BYTES, MAX_NAMES, MAX_VALUE_CHARS, PackageJsonFacts, PyprojectFacts,
    Repository,
};
use crate::paths::dirfd;

#[cfg(test)]
mod tests;

/// Read the facts of the open repository `dir` at `path`. A fact that cannot be read becomes a
/// note.
pub(super) fn describe(dir: BorrowedFd<'_>, path: &Path) -> Repository {
    let mut notes = Vec::new();
    let names = match dirfd::read_dir_entries(dir) {
        Ok(entries) => sorted_names(
            entries.iter().map(|e| {
                let name = e.name.to_string_lossy();
                if e.is_dir() {
                    format!("{name}/")
                } else {
                    name.into_owned()
                }
            }),
            "the top level",
            &mut notes,
        ),
        Err(e) => {
            notes.push(format!("cannot list the top level: {e}"));
            Vec::new()
        }
    };
    let workflows = workflow_names(dir, &mut notes);
    let cargo = read_manifest(dir, "Cargo.toml", &mut notes)
        .and_then(|text| parse_toml(&text, "Cargo.toml", &mut notes))
        .map(|table| cargo_facts(&table, &mut notes));
    let package_json = read_manifest(dir, "package.json", &mut notes).and_then(|text| {
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|e| notes.push(format!("package.json does not parse: {e}")))
            .ok()
            .map(|value| package_json_facts(&value, &mut notes))
    });
    let pyproject = read_manifest(dir, "pyproject.toml", &mut notes)
        .and_then(|text| parse_toml(&text, "pyproject.toml", &mut notes))
        .map(|table| pyproject_facts(&table, &mut notes));
    Repository {
        path: path.to_path_buf(),
        names,
        workflows,
        cargo,
        package_json,
        pyproject,
        notes,
    }
}

/// The `.yml` and `.yaml` names under `.github/workflows/`. Both folders are opened without
/// following a symlink, so a linked `.github` cannot list a folder outside the repository.
fn workflow_names(dir: BorrowedFd<'_>, notes: &mut Vec<String>) -> Vec<String> {
    let opened = dirfd::open_dir_at(dir, OsStr::new(".github")).and_then(|github| {
        dirfd::open_dir_at(github.as_fd(), OsStr::new("workflows"))
            .and_then(|workflows| dirfd::read_dir_entries(workflows.as_fd()))
    });
    match opened {
        Ok(entries) => sorted_names(
            entries
                .iter()
                .map(|e| e.name.to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".yml") || name.ends_with(".yaml")),
            ".github/workflows",
            notes,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            notes.push(format!(
                "cannot list .github/workflows (a symlink is not followed): {e}"
            ));
            Vec::new()
        }
    }
}

/// Cut each name, sort them, and keep the first [`MAX_NAMES`]. A longer list leaves a note.
fn sorted_names(
    names: impl Iterator<Item = String>,
    what: &str,
    notes: &mut Vec<String>,
) -> Vec<String> {
    let mut names: Vec<String> = names.map(|name| cut(&name)).collect();
    names.sort();
    if names.len() > MAX_NAMES {
        notes.push(format!(
            "{what} lists {} names; only the first MAX_NAMES ({MAX_NAMES}) are kept",
            names.len()
        ));
        names.truncate(MAX_NAMES);
    }
    names
}

/// Read the manifest `name` from `dir`. A missing file is `None` with no note. A symlink, a file
/// that is not a regular file, a file over [`MAX_MANIFEST_BYTES`], or a read error is `None`
/// with a note.
fn read_manifest(dir: BorrowedFd<'_>, name: &str, notes: &mut Vec<String>) -> Option<String> {
    match read_regular_file(dir, name) {
        Ok(text) => text,
        Err(note) => {
            notes.push(format!("{name}: {note}"));
            None
        }
    }
}

/// Open `name` once without following a symlink and without blocking on a FIFO, check that it
/// is a regular file, and read at most [`MAX_MANIFEST_BYTES`] from it. The checks and the read
/// use the same open file, so a swap between them cannot change what is read.
fn read_regular_file(dir: BorrowedFd<'_>, name: &str) -> Result<Option<String>, String> {
    let bytes = match crate::paths::dirfd::read_file_at(dir, name.as_ref(), MAX_MANIFEST_BYTES) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(Errno::LOOP.raw_os_error()) => {
            return Err("it is a symlink, which the scan does not follow".into());
        }
        Err(e) => return Err(e.to_string()),
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "it is not UTF-8 text".into())
}

fn parse_toml(text: &str, name: &str, notes: &mut Vec<String>) -> Option<toml::Table> {
    text.parse::<toml::Table>()
        .map_err(|e| notes.push(format!("{name} does not parse: {}", e.message())))
        .ok()
}

/// The string at `table[keys...]`, cut to [`MAX_VALUE_CHARS`]. A `{ workspace = true }` value,
/// which a workspace member uses to inherit the key, reads as `"workspace"`. Any other type
/// leaves a note that names the key.
fn string_at(table: &toml::Table, keys: &[&str], notes: &mut Vec<String>) -> Option<String> {
    let (last, parents) = keys.split_last()?;
    let mut current = table;
    for key in parents {
        current = current.get(*key)?.as_table()?;
    }
    let value = current.get(*last)?;
    if let Some(text) = value.as_str() {
        return Some(cut(text));
    }
    let inherited = value
        .as_table()
        .and_then(|t| t.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    if inherited {
        return Some("workspace".to_string());
    }
    notes.push(format!(
        "{} is a {}, not a string",
        keys.join("."),
        value.type_str()
    ));
    None
}

/// The first [`MAX_VALUE_CHARS`] characters of `value`.
fn cut(value: &str) -> String {
    value.chars().take(MAX_VALUE_CHARS).collect()
}

fn cargo_facts(table: &toml::Table, notes: &mut Vec<String>) -> CargoFacts {
    CargoFacts {
        name: string_at(table, &["package", "name"], notes),
        edition: string_at(table, &["package", "edition"], notes),
        rust_version: string_at(table, &["package", "rust-version"], notes),
        workspace: table.contains_key("workspace"),
    }
}

fn package_json_facts(value: &serde_json::Value, notes: &mut Vec<String>) -> PackageJsonFacts {
    let scripts = value
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .map(|scripts| sorted_names(scripts.keys().cloned(), "package.json scripts", notes))
        .unwrap_or_default();
    PackageJsonFacts {
        scripts,
        package_manager: value
            .get("packageManager")
            .and_then(serde_json::Value::as_str)
            .map(cut),
    }
}

fn pyproject_facts(table: &toml::Table, notes: &mut Vec<String>) -> PyprojectFacts {
    let tools = table
        .get("tool")
        .and_then(toml::Value::as_table)
        .map(|tools| sorted_names(tools.keys().cloned(), "pyproject.toml [tool]", notes))
        .unwrap_or_default();
    PyprojectFacts {
        name: string_at(table, &["project", "name"], notes),
        requires_python: string_at(table, &["project", "requires-python"], notes),
        build_backend: string_at(table, &["build-system", "build-backend"], notes),
        tools,
    }
}
