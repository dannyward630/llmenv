//! Read of the `.llmenv.yaml` project marker file.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Schema for the body of `.llmenv.yaml` (project marker file).
/// All fields optional; an empty file is valid.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProjectFile {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) enable_bundles: Vec<String>,
    #[serde(default)]
    pub(crate) disable_bundles: Vec<String>,
    /// Capture unknown fields for warning emission.
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, serde_yaml::Value>,
}

/// Maximum length (in bytes) for the project description. Anything longer
/// is truncated and a warning is logged. The description is surfaced into
/// LLM context chunks; a hard cap prevents a malformed or hostile marker
/// from bloating every prompt.
pub(crate) const MAX_DESCRIPTION_BYTES: usize = 1024;

/// The largest `.llmenv.yaml` that llmenv reads. A cloned repository controls the file, and a
/// marker holds a few short fields, so a larger file is a mistake or an attack (#2942).
pub(crate) const MAX_MARKER_FILE_BYTES: u64 = 64 * 1024;

/// Parse `.llmenv.yaml` file into a `ProjectFile`. Empty file → all defaults.
/// Malformed YAML → log warning and return defaults. The `description`
/// field is truncated to `MAX_DESCRIPTION_BYTES` if oversized.
pub(crate) fn read_project_file(path: &std::path::Path) -> ProjectFile {
    let body = match llmenv_paths::read_text_bounded(path, MAX_MARKER_FILE_BYTES) {
        Ok(Some(body)) => body,
        Ok(None) => return ProjectFile::default(),
        Err(e) => {
            tracing::error!(
                path = %path.display(),
                error = %e,
                "failed to read project marker file; using defaults"
            );
            return ProjectFile::default();
        }
    };
    if body.trim().is_empty() {
        return ProjectFile::default();
    }
    match serde_yaml::from_str::<ProjectFile>(&body) {
        Ok(mut pf) => {
            if let Some(desc) = pf.description.as_mut()
                && desc.len() > MAX_DESCRIPTION_BYTES
            {
                tracing::warn!(
                    "project marker file {} has description >{} bytes; truncating",
                    path.display(),
                    MAX_DESCRIPTION_BYTES
                );
                // Truncate at a char boundary so the result remains valid UTF-8.
                let mut cut = MAX_DESCRIPTION_BYTES;
                while cut > 0 && !desc.is_char_boundary(cut) {
                    cut -= 1;
                }
                desc.truncate(cut);
            }
            pf
        }
        Err(e) => {
            tracing::error!(
                "project marker file {} is not valid YAML: {e}; using defaults",
                path.display()
            );
            ProjectFile::default()
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn malformed_project_file_is_logged_at_error_level() {
        // #2666: the default log filter is ERROR, so a warn here never reaches the user.
        let temp_dir = tempfile::TempDir::new().expect("tempdir");
        let marker = temp_dir.path().join(".llmenv.yaml");
        std::fs::write(&marker, "description: [unclosed\n").expect("write marker");
        let logs = crate::test_log_capture::capture_logs(|| {
            assert_eq!(read_project_file(&marker).description, None);
        });
        assert!(logs.contains("ERROR"), "expected an ERROR line: {logs}");
        assert!(logs.contains("not valid YAML"), "{logs}");
    }

    #[test]
    fn a_fifo_or_oversized_marker_gives_defaults_at_once() {
        let temp_dir = tempfile::TempDir::new().expect("tempdir");
        let marker = temp_dir.path().join(".llmenv.yaml");
        let made = std::process::Command::new("mkfifo")
            .arg(&marker)
            .status()
            .expect("mkfifo");
        assert!(made.success());
        assert_eq!(read_project_file(&marker).id, None);
        std::fs::remove_file(&marker).expect("remove fifo");
        let max = usize::try_from(MAX_MARKER_FILE_BYTES).expect("cap fits usize");
        let mut body = b"id: big\n".to_vec();
        body.resize(max + 1, b'#');
        std::fs::write(&marker, &body).expect("write marker");
        assert_eq!(read_project_file(&marker).id, None);
        body.truncate(max);
        std::fs::write(&marker, &body).expect("write marker");
        assert_eq!(read_project_file(&marker).id.as_deref(), Some("big"));
    }
}
