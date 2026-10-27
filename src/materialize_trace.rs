//! `[LLMENV_MATERIALIZE]` trace line for adapter materialize steps.
//!
//! These functions emit a trace line with file counts and bytes when
//! `LLMENV_TRACE_TIMING` is set. They are separated from `trace.rs` so
//! that `trace.rs` stays near its line cap.

use crate::trace::emit_trace_line;

/// Emit a `[LLMENV_MATERIALIZE] <adapter> written=<n> unchanged=<n> removed=<n> bytes=<n> <us>us`
/// line to the trace sink.
///
/// The function lists all files in the adapter cache directory, excludes metadata
/// files (`llmenv-status.json`, `.llmenv-manifest.json`), and compares the result
/// against the manifest to compute written / unchanged / removed counts and the
/// total bytes of written files.
fn emit_materialize_trace(
    adapter_name: &str,
    cache_path: &std::path::Path,
    manifest_file_names: &[&str],
    written_file_names: &[&str],
    elapsed: std::time::Duration,
) {
    if std::env::var_os("LLMENV_TRACE_TIMING").is_none() {
        return;
    }
    // Metadata files that should not count toward materialize stats.
    let meta: &[&str] = &["llmenv-status.json", ".llmenv-manifest.json"];
    // All files in the cache directory, excluding metadata, as owned names.
    let cache_names: Vec<String> = match std::fs::read_dir(cache_path) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if meta.contains(&name.as_str()) {
                    return None;
                }
                Some(name)
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let mut written_bytes: u64 = 0;
    for name in written_file_names {
        let path = cache_path.join(*name);
        if let Ok(fmeta) = std::fs::metadata(&path)
            && fmeta.is_file()
        {
            written_bytes += fmeta.len();
        }
    }
    // Files in cache but not in manifest = removed.
    let removed: usize = cache_names
        .iter()
        .filter(|n| !manifest_file_names.contains(&n.as_str()))
        .count();
    let unchanged: usize = manifest_file_names
        .iter()
        .filter(|n| {
            let path = cache_path.join(**n);
            std::fs::metadata(&path).is_ok_and(|m| m.is_file())
        })
        .count();
    let written = written_file_names.len();
    let us = elapsed.as_micros();
    emit_trace_line(&format!(
        "[LLMENV_MATERIALIZE] {adapter_name} \
         written={written} unchanged={unchanged} removed={removed} \
         bytes={written_bytes} {us}us"
    ));
}

/// Record materialize file counts for one adapter and emit the trace line.
///
/// This is a thin wrapper that extracts file names from the manifest and
/// adapter-owned paths, then calls `emit_materialize_trace`. Call this
/// after `adapter.materialize()` returns.
pub(crate) fn record_materialize(
    adapter: &dyn crate::adapter::AgentAdapter,
    cache_path: &std::path::Path,
    manifest: &crate::merge::MergedManifest,
    adapter_owned: &[std::path::PathBuf],
    elapsed: std::time::Duration,
) {
    // Collect into owned strings to avoid lifetime issues with iterator temporaries.
    let manifest_names: Vec<String> = manifest
        .files
        .keys()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let written_names: Vec<String> = adapter_owned
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    emit_materialize_trace(
        adapter.name(),
        cache_path,
        // Safe: file names from the manifest and adapter are always valid UTF-8 on POSIX.
        &manifest_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        &written_names.iter().map(String::as_str).collect::<Vec<_>>(),
        elapsed,
    );
}
