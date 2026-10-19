//! Settings that llmenv seeds into `settings.json` when they are absent: user-elected keys,
//! `installMethod`, and the default `statusLine`.

use std::path::Path;

use super::LLMENV_OWNED_SETTINGS_KEYS;

/// Merge user-elected seeded keys into `out/settings.json` after the adapter
/// has already written the file (#172). Runs on every render, not just new
/// folders: `reconcile_settings` already preserves existing foreign keys, so
/// for re-renders this is nearly always a no-op (all seeded keys already
/// present). For a fresh folder, this adds user defaults that would otherwise
/// be absent from the first-rendered `settings.json`.
///
/// **Must be called after `adapter.materialize()`** so that if materialize
/// fails, settings.json is left either absent (new folder, no partial state)
/// or in its prior good state (re-render, reconcile is atomic). Calling before
/// materialize can leave a seeded-only settings.json (no llmenv-owned keys)
/// if materialize subsequently errors.
///
/// # Errors
/// Returns an error if serialization or the atomic write fails.
pub(crate) fn apply_seeded_settings(
    out: &Path,
    seeded: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<()> {
    if seeded.is_empty() {
        return Ok(());
    }
    let path = out.join("settings.json");
    // Read whatever materialize wrote; no-op if file absent (materialize
    // failed or skipped — don't create a seeded-only file in that case).
    let existing: serde_json::Value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            anyhow::anyhow!(
                "{} is not valid JSON ({e}), so the seeded settings were not applied",
                path.display()
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "reading {} for seeding: {e}",
                path.display()
            ));
        }
    };
    let serde_json::Value::Object(mut obj) = existing else {
        anyhow::bail!(
            "{} is not a JSON object, so the seeded settings were not applied",
            path.display()
        );
    };
    let mut changed = false;
    for (k, v) in seeded {
        // Never add llmenv-owned keys — reconcile_settings owns those.
        if !LLMENV_OWNED_SETTINGS_KEYS.contains(&k.as_str()) && !obj.contains_key(k) {
            obj.insert(k.clone(), v.clone());
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(obj))?;
    crate::paths::write_user_file_atomic(&path, json.as_bytes())
        .map_err(|e| anyhow::anyhow!("writing seeded settings {}: {e}", path.display()))
}

/// Classify a `claude` binary path as `"homebrew"`, `"npm"`, or `"native"`.
pub(super) fn classify_claude_path(path: &str) -> &'static str {
    let lc = path.to_ascii_lowercase();
    if lc.contains("/homebrew/") || lc.contains("/cellar/") || lc.contains("/linuxbrew/") {
        "homebrew"
    } else if lc.contains("node_modules")
        || lc.contains("/.npm")
        || lc.contains("/.nvm")
        || lc.contains("/npm/")
        || lc.contains("/.volta/")
        || lc.contains("/.fnm/")
        || lc.contains("/.local/share/pnpm/")
        || lc.contains("/library/pnpm/")
    {
        "npm"
    } else {
        "native"
    }
}

/// The path of the `claude` program on `PATH`, looked up without a process, so the lookup cannot
/// hang or fail to start (#2923).
fn find_claude_binary() -> Option<String> {
    find_claude_binary_in(std::env::var_os("PATH").as_deref())
}

fn find_claude_binary_in(path_var: Option<&std::ffi::OsStr>) -> Option<String> {
    crate::path_lookup::find_on_path("claude", path_var).map(|path| path.display().to_string())
}

#[cfg(test)]
#[test]
#[expect(clippy::expect_used, reason = "test code")]
fn the_claude_lookup_finds_the_program_in_an_absolute_folder_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("test");
    let claude = dir.path().join("claude");
    std::fs::write(&claude, "#!/bin/sh\n").expect("test");
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).expect("test");
    let path_var = std::env::join_paths([dir.path()]).expect("test");
    assert_eq!(
        find_claude_binary_in(Some(&path_var)),
        Some(claude.display().to_string())
    );
    assert_eq!(find_claude_binary_in(None), None);
    let empty = std::env::join_paths([dir.path().join("none")]).expect("test");
    assert_eq!(find_claude_binary_in(Some(&empty)), None);
}

/// Seed `installMethod` into `out/settings.json` if absent (#346).
///
/// Detects how `claude` was installed by inspecting its binary path, then
/// writes the result as a foreign key so it survives every re-render.
/// No-op if `settings.json` does not exist (materialize hasn't run yet) or if
/// `installMethod` is already present.
///
/// # Errors
/// Returns an error if the file exists but cannot be read or written.
pub(crate) fn seed_install_method(out: &std::path::Path) -> anyhow::Result<()> {
    let settings_path = out.join("settings.json");

    // Skip fork if installMethod already present.
    match std::fs::read(&settings_path) {
        Ok(bytes) => {
            if let Ok(serde_json::Value::Object(obj)) =
                serde_json::from_slice::<serde_json::Value>(&bytes)
                && obj.contains_key("installMethod")
            {
                return Ok(());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist yet, that's fine.
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "reading {} for seeding installMethod: {e}",
                settings_path.display()
            ));
        }
    }

    let method = find_claude_binary()
        .as_deref()
        .map_or("native", classify_claude_path);
    let mut seeded = serde_json::Map::new();
    seeded.insert("installMethod".to_string(), serde_json::Value::from(method));
    apply_seeded_settings(out, &seeded)
}

/// Seed the default `statusLine` hook into `out/settings.json` if absent
/// (#836), pointing Claude Code's statusline at `llmenv statusline`.
///
/// Deliberately a **seed**, not an owned/authoritative render key (contrast
/// `autoMemoryEnabled`): `statusLine` is not in [`LLMENV_OWNED_SETTINGS_KEYS`],
/// so if this were instead emitted unconditionally into `render_settings`'s
/// `fresh` map, `reconcile_settings`'s passthrough rule would write it
/// through on *every* re-render — permanently clobbering a user's own
/// `/statusline` customization (which writes straight to `settings.json`,
/// outside llmenv) on the very next shell-hook materialize. Seeding once,
/// only when absent, matches [`seed_install_method`]'s contract: once any
/// value exists — llmenv's default, a user customization, or a
/// `native.claude_code.statusLine` override already written by
/// `reconcile_settings` before this runs — it is left alone.
///
/// No-op if `settings.json` does not exist yet (materialize hasn't run) or if
/// `statusLine` is already present.
///
/// # Errors
/// Returns an error if the file exists but cannot be read or written.
pub(crate) fn seed_status_line(out: &std::path::Path) -> anyhow::Result<()> {
    let settings_path = out.join("settings.json");

    match std::fs::read(&settings_path) {
        Ok(bytes) => {
            if let Ok(serde_json::Value::Object(obj)) =
                serde_json::from_slice::<serde_json::Value>(&bytes)
                && obj.contains_key("statusLine")
            {
                return Ok(());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File doesn't exist yet, that's fine.
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "reading {} for seeding statusLine: {e}",
                settings_path.display()
            ));
        }
    }

    let mut seeded = serde_json::Map::new();
    seeded.insert(
        "statusLine".to_string(),
        // `--color always`: Claude Code invokes this with stdout captured
        // (never a TTY), so the default `auto` color resolution would
        // silently disable every `style:` widget override — the seeded
        // command must opt in explicitly rather than rely on TTY detection.
        serde_json::json!({ "type": "command", "command": "llmenv statusline --color always" }),
    );
    apply_seeded_settings(out, &seeded)
}
