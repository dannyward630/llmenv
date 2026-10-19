//! The facts that the scan reads from one repository: top-level names, workflow names, and a
//! fixed set of manifest keys (#2785).

use std::path::Path;

use super::{
    CargoFacts, MAX_MANIFEST_BYTES, MAX_NAMES, MAX_VALUE_CHARS, PackageJsonFacts, PyprojectFacts,
    Repository,
};

/// Read the facts of the repository at `path`. A fact that cannot be read becomes a note.
pub(super) fn describe(path: &Path) -> Repository {
    let mut notes = Vec::new();
    let names = listing(path, &mut notes, |_| true);
    let workflows_dir = path.join(".github").join("workflows");
    let workflows = if is_real_folder(&workflows_dir) {
        listing(&workflows_dir, &mut notes, |name| {
            name.ends_with(".yml") || name.ends_with(".yaml")
        })
    } else {
        Vec::new()
    };
    let cargo = read(path, "Cargo.toml", &mut notes)
        .and_then(|text| parse_toml(&text, "Cargo.toml", &mut notes))
        .map(|table| cargo_facts(&table));
    let package_json = read(path, "package.json", &mut notes).and_then(|text| {
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|e| notes.push(format!("package.json does not parse: {e}")))
            .ok()
            .map(|value| package_json_facts(&value))
    });
    let pyproject = read(path, "pyproject.toml", &mut notes)
        .and_then(|text| parse_toml(&text, "pyproject.toml", &mut notes))
        .map(|table| pyproject_facts(&table));
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

/// The sorted names in `dir` that pass `keep`, at most [`MAX_NAMES`]. A folder name ends in `/`.
fn listing(dir: &Path, notes: &mut Vec<String>, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            notes.push(format!("cannot list {}: {e}", dir.display()));
            return Vec::new();
        }
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| {
            entry
                .map_err(|e| notes.push(format!("an entry of {}: {e}", dir.display())))
                .ok()
        })
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_folder = entry.file_type().is_ok_and(|t| t.is_dir());
            keep(&name).then(|| if is_folder { format!("{name}/") } else { name })
        })
        .collect();
    names.sort();
    if names.len() > MAX_NAMES {
        notes.push(format!(
            "{} lists {} names; only the first MAX_NAMES ({MAX_NAMES}) are kept",
            dir.display(),
            names.len()
        ));
        names.truncate(MAX_NAMES);
    }
    names
}

/// Whether `path` is a folder and not a symlink to one.
fn is_real_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Read the manifest `name` in `repo`. A missing file is `None` with no note. A symlink, a file
/// over [`MAX_MANIFEST_BYTES`], or a read error is `None` with a note.
fn read(repo: &Path, name: &str, notes: &mut Vec<String>) -> Option<String> {
    let path = repo.join(name);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            notes.push(format!("{name}: {e}"));
            return None;
        }
    };
    if meta.file_type().is_symlink() {
        notes.push(format!(
            "{name} is a symlink, which the scan does not follow"
        ));
        return None;
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        notes.push(format!(
            "{name} holds {} bytes, more than MAX_MANIFEST_BYTES ({MAX_MANIFEST_BYTES})",
            meta.len()
        ));
        return None;
    }
    std::fs::read_to_string(&path)
        .map_err(|e| notes.push(format!("{name}: {e}")))
        .ok()
}

fn parse_toml(text: &str, name: &str, notes: &mut Vec<String>) -> Option<toml::Table> {
    text.parse::<toml::Table>()
        .map_err(|e| notes.push(format!("{name} does not parse: {}", e.message())))
        .ok()
}

/// The string at `table[keys...]`, cut to [`MAX_VALUE_CHARS`].
fn string_at(table: &toml::Table, keys: &[&str]) -> Option<String> {
    let (last, parents) = keys.split_last()?;
    let mut current = table;
    for key in parents {
        current = current.get(*key)?.as_table()?;
    }
    current.get(*last)?.as_str().map(cut)
}

fn cut(value: &str) -> String {
    value.chars().take(MAX_VALUE_CHARS).collect()
}

fn cargo_facts(table: &toml::Table) -> CargoFacts {
    CargoFacts {
        name: string_at(table, &["package", "name"]),
        edition: string_at(table, &["package", "edition"]),
        rust_version: string_at(table, &["package", "rust-version"]),
        workspace: table.contains_key("workspace"),
    }
}

fn package_json_facts(value: &serde_json::Value) -> PackageJsonFacts {
    let mut scripts: Vec<String> = value
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .map(|scripts| scripts.keys().map(|k| cut(k)).collect())
        .unwrap_or_default();
    scripts.sort();
    scripts.truncate(MAX_NAMES);
    PackageJsonFacts {
        scripts,
        package_manager: value
            .get("packageManager")
            .and_then(serde_json::Value::as_str)
            .map(cut),
    }
}

fn pyproject_facts(table: &toml::Table) -> PyprojectFacts {
    let mut tools: Vec<String> = table
        .get("tool")
        .and_then(toml::Value::as_table)
        .map(|tools| tools.keys().map(|k| cut(k)).collect())
        .unwrap_or_default();
    tools.sort();
    tools.truncate(MAX_NAMES);
    PyprojectFacts {
        name: string_at(table, &["project", "name"]),
        requires_python: string_at(table, &["project", "requires-python"]),
        build_backend: string_at(table, &["build-system", "build-backend"]),
        tools,
    }
}
