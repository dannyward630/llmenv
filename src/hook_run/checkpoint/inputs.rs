//! Recovery of a detached job's inputs from its checkpoint when stdin is unusable (#2779).

use super::*;

/// Why the inputs could not be recovered.
///
/// `keep_checkpoint` is true when the checkpoint exists but could not be read. A read failure can
/// be transient, so a retry may still use the file. A corrupt checkpoint and a missing one are not
/// kept, as before.
#[derive(Debug)]
pub(crate) struct InputsError {
    error: anyhow::Error,
    pub(crate) keep_checkpoint: bool,
}

impl InputsError {
    pub(crate) fn into_error(self) -> anyhow::Error {
        self.error
    }
}

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
) -> Result<T, InputsError> {
    let stdin_error = match serde_json::from_str::<T>(stdin_text) {
        Ok(inputs) => return Ok(inputs),
        Err(e) => e,
    };
    match recorded_inputs::<T>(path) {
        Ok(Some(inputs)) => {
            tracing::error!("job input on stdin is unusable ({stdin_error}); using the checkpoint");
            Ok(inputs)
        }
        Ok(None) => Err(InputsError {
            error: stdin_error.into(),
            keep_checkpoint: false,
        }),
        Err(failure) => {
            tracing::error!("job checkpoint is unusable: {failure}");
            Err(InputsError {
                error: anyhow::anyhow!(
                    "job input on stdin is unusable ({stdin_error}), and the checkpoint is \
                     unusable too ({failure}). Fix: remove the checkpoint file, or re-run the job."
                ),
                keep_checkpoint: failure.keeps_file(),
            })
        }
    }
}

/// A checkpoint that exists but yields no inputs. A read failure keeps the file, a parse failure
/// does not.
enum CheckpointFailure {
    Read(String),
    Parse(String),
}

impl CheckpointFailure {
    fn keeps_file(&self) -> bool {
        matches!(self, Self::Read(_))
    }
}

impl std::fmt::Display for CheckpointFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(msg) | Self::Parse(msg) => f.write_str(msg),
        }
    }
}

/// The inputs in the checkpoint at `path`. `Ok(None)` means no checkpoint exists yet, which is
/// normal before the first save.
fn recorded_inputs<T: serde::de::DeserializeOwned>(
    path: Option<&Path>,
) -> Result<Option<T>, CheckpointFailure> {
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
        Err(e) if e.downcast_ref::<serde_json::Error>().is_some() => {
            return Err(CheckpointFailure::Parse(format!("{e:#}")));
        }
        Err(e) => return Err(CheckpointFailure::Read(format!("{e:#}"))),
    };
    serde_json::from_value::<T>(checkpoint.inputs)
        .map(Some)
        .map_err(|e| CheckpointFailure::Parse(format!("inputs do not parse: {e}")))
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
        assert!(!err.keep_checkpoint, "a corrupt checkpoint is not kept");
        let msg = format!("{:#}", err.into_error());
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
        assert!(!err.keep_checkpoint);
        let msg = format!("{:#}", err.into_error());
        assert!(
            !msg.contains("checkpoint"),
            "a missing checkpoint must not be named: {msg}"
        );
    }

    #[test]
    fn unreadable_checkpoint_keeps_its_file() {
        // A checkpoint path that cannot be read as a file is an I/O failure, not corruption. A
        // retry may still read it, so the caller must not delete it (#2779).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cp.json");
        std::fs::create_dir(&path).unwrap();
        let err = inputs_or_checkpoint::<serde_json::Value>("{not json", Some(&path)).unwrap_err();
        assert!(err.keep_checkpoint, "an unreadable checkpoint must be kept");
    }
}
