//! The temp folder that an engine and its subprocesses use (#1379).
//!
//! It is in the durable state dir, not in the cache folder, because `llmenv clean` and a config
//! edit can delete a cache folder while a shell still points at it. Then a later temp file fails
//! with an error that names neither llmenv nor `TMPDIR`, such as a failed SSH-signed commit.

use std::path::Path;

/// Create `<state_dir>/tmp`, and return its path with `TMPDIR`, `TMP`, and `TEMP` set to it.
///
/// # Errors
/// The folder cannot be created, or its path is not valid UTF-8.
pub(crate) fn temp_dir_vars(state_dir: &Path) -> anyhow::Result<(String, Vec<(String, String)>)> {
    let tmp_dir = state_dir.join("tmp");
    std::fs::create_dir_all(&tmp_dir)?;
    let tmp = tmp_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("tmp dir is not valid UTF-8: {}", tmp_dir.display()))?
        .to_owned();
    let vars = ["TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .map(|name| (name.to_owned(), tmp.clone()))
        .collect();
    Ok((tmp, vars))
}
