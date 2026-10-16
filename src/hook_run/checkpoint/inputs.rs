//! Recovery of a detached job's inputs from its checkpoint when stdin is unusable (#2779).

use super::*;

/// The inputs of a detached job. `stdin_text` is what the parent piped to the child. When that
/// text does not parse as `T`, for example because the parent failed to write all of it, the inputs
/// that the parent recorded in the checkpoint at `path` are used. The job then runs, and a
/// truncated pipe does not lose it.
///
/// # Errors
/// The text does not parse, and there is no readable checkpoint whose inputs parse.
pub(crate) fn inputs_or_checkpoint<T: serde::de::DeserializeOwned>(
    stdin_text: &str,
    path: Option<&Path>,
) -> anyhow::Result<T> {
    let stdin_error = match serde_json::from_str::<T>(stdin_text) {
        Ok(inputs) => return Ok(inputs),
        Err(e) => e,
    };
    match recorded_inputs::<T>(path) {
        Ok(Some(inputs)) => {
            tracing::error!("job input on stdin is unusable ({stdin_error}); using the checkpoint");
            Ok(inputs)
        }
        Ok(None) => Err(stdin_error.into()),
        Err(checkpoint_error) => {
            tracing::error!("job checkpoint is unusable: {checkpoint_error}");
            Err(anyhow::anyhow!(
                "job input on stdin is unusable ({stdin_error}), and the checkpoint is unusable \
                 too ({checkpoint_error}). Fix: remove the checkpoint file, or re-run the job."
            ))
        }
    }
}

/// The inputs in the checkpoint at `path`. `Ok(None)` means no checkpoint exists yet, which is
/// normal before the first save. Any other read or parse failure is an error (#2779).
fn recorded_inputs<T: serde::de::DeserializeOwned>(
    path: Option<&Path>,
) -> Result<Option<T>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let checkpoint = match load(path) {
        Ok(checkpoint) => checkpoint,
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(format!("{e:#}")),
    };
    serde_json::from_value::<T>(checkpoint.inputs)
        .map(Some)
        .map_err(|e| format!("inputs do not parse: {e}"))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::inputs_or_checkpoint;

    #[test]
    fn inputs_error_names_a_corrupt_checkpoint_too() {
        // A checkpoint that exists but does not parse must be named, not dropped (#2779).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cp.json");
        std::fs::write(&path, "{not json").unwrap();
        let err = inputs_or_checkpoint::<serde_json::Value>("{not json", Some(&path)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("checkpoint"),
            "error must name the checkpoint: {msg}"
        );
    }

    #[test]
    fn inputs_error_is_the_stdin_error_when_no_checkpoint_exists() {
        // A missing checkpoint is normal before the first save, so it adds nothing to the error.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.json");
        let err = inputs_or_checkpoint::<serde_json::Value>("{not json", Some(&path)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            !msg.contains("checkpoint"),
            "a missing checkpoint must not be named: {msg}"
        );
    }
}
