//! Reads of a bundle's `AGENTS.md` and `bundle.yaml` (#2933).

use std::io::Read as _;
use std::path::Path;

use crate::config::Capabilities;

use rustix::fs::{Mode, OFlags};

/// The largest bundle text file that a merge reads. The text goes into every session, so a file
/// this large is a mistake, not content.
const MAX_BUNDLE_FILE_BYTES: u64 = 1024 * 1024;

/// Read the bundle text file at `path`, or `None` when there is none. A link is followed, because
/// a linked `AGENTS.md` can be deliberate. The open does not block on a FIFO, and the open file
/// must be a regular file of at most [`MAX_BUNDLE_FILE_BYTES`] bytes of UTF-8 text. The checks
/// and the read use the same open file, so a swap between them cannot change what is read.
///
/// # Errors
/// The file cannot be opened or read, is not a regular file, is too large, or is not UTF-8.
pub(crate) fn read_bundle_text(path: &Path) -> std::io::Result<Option<String>> {
    let flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let fd = match rustix::fs::open(path, flags, Mode::empty()) {
        Ok(fd) => fd,
        // A link whose target is gone also gives ENOENT. Its bundle meant to have the file, so
        // that is an error, not an absent file.
        Err(rustix::io::Errno::NOENT) if rustix::fs::lstat(path).is_ok() => {
            return Err(invalid(
                "it is a link to a file that does not exist".to_string(),
            ));
        }
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let st = rustix::fs::fstat(&fd)?;
    let kind = rustix::fs::FileType::from_raw_mode(st.st_mode as rustix::fs::RawMode);
    if kind != rustix::fs::FileType::RegularFile {
        return Err(invalid(format!("it is not a regular file ({kind:?})")));
    }
    let mut bytes = Vec::new();
    std::fs::File::from(fd)
        .take(MAX_BUNDLE_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BUNDLE_FILE_BYTES {
        return Err(invalid(format!(
            "it holds more than {MAX_BUNDLE_FILE_BYTES} bytes"
        )));
    }
    String::from_utf8(bytes).map(Some).map_err(|e| {
        invalid(format!(
            "it is not UTF-8 text (the first bad byte is at offset {})",
            e.utf8_error().valid_up_to()
        ))
    })
}

fn invalid(reason: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, reason)
}

/// Keys that a `bundle.yaml` fragment is allowed to declare. Any other top-level
/// key is rejected with a hard error rather than silently dropped.
pub(super) const BUNDLE_YAML_KNOWN_KEYS: &[&str] = &[
    "permissions",
    "hooks",
    "plugins",
    "mcp",
    "lsp",
    "skills",
    "output_styles",
    "env",
    "auto_memory_enabled",
    "effort_level",
    "model_effort",
    "advisor_model",
    // Parsed so validation can name the replacement (#2409); never merged.
    "advisor_size",
    "native_permissions",
    "native_hooks",
    "native_plugins",
    "native_mcp",
    "native_model_providers",
    "native_default_models",
    "native",
    "features",
    "host",
    // Found during #1031: these two were missing despite Capabilities' own doc
    // comments documenting bundle-level support ("contributed by this
    // capability source (bundle or top-level)") — any bundle.yaml declaring
    // either was hard-rejected as an unknown key, with no test ever
    // exercising the documented claim.
    "model_providers",
    "default_models",
];

/// Read an optional `bundle.yaml` capability fragment from a bundle directory.
/// Returns `None` when the file is absent — bundles carry capabilities only if
/// they choose to.
///
/// `pub(crate)`: also called by `hook_run`'s memory diagnostics, which need one
/// suppressed bundle's declarations without merging it into the manifest.
///
/// # Errors
/// An unreadable file, invalid YAML, or an unknown top-level key.
pub(crate) fn read_bundle_yaml(
    bundle_root: &Path,
    name: &str,
) -> anyhow::Result<Option<Capabilities>> {
    let path = bundle_root.join("bundle.yaml");
    let s = match read_bundle_text(&path) {
        Ok(Some(s)) => s,
        Ok(None) => return Ok(None),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "bundle '{name}': reading {}: {e}",
                path.display()
            ));
        }
    };
    let raw: serde_yaml::Value = serde_yaml::from_str(&s)
        .map_err(|e| anyhow::anyhow!("bundle '{name}': parsing {}: {e}", path.display()))?;
    if let Some(mapping) = raw.as_mapping() {
        for key in mapping.keys() {
            if let Some(k) = key.as_str()
                && !BUNDLE_YAML_KNOWN_KEYS.contains(&k)
            {
                anyhow::bail!(
                    "bundle '{name}': unknown key '{k}' in bundle.yaml — \
                     known keys: {}",
                    BUNDLE_YAML_KNOWN_KEYS.join(", ")
                );
            }
        }
    }
    let mut caps: Capabilities = serde_yaml::from_value(raw)
        .map_err(|e| anyhow::anyhow!("bundle '{name}': parsing {}: {e}", path.display()))?;

    // Track which bundle each hook came from, so the adapter can resolve relative paths later.
    // We don't resolve paths here because duplicate hooks (e.g., "hooks/guard.sh" from two bundles)
    // must dedup correctly before being adapted into settings.json.
    for hook in &mut caps.hooks {
        hook.bundle_origin = Some(bundle_root.to_path_buf());
    }

    let context = format!("bundle '{name}'");
    for key in caps.env.keys() {
        crate::config::validate_capabilities_env_key(&context, key)?;
    }
    llmenv_config::validate_effort(&context, &caps)?;
    llmenv_config::validate_advisor(&context, &caps)?;
    let permission_rules = caps
        .permissions
        .allow
        .iter()
        .chain(caps.permissions.ask.iter())
        .chain(caps.permissions.deny.iter());
    for rule in permission_rules {
        crate::config::validate_permission_rule(&context, rule)?;
    }
    for (engine, nr) in &caps.native_permissions {
        let ctx = format!("bundle '{name}': native_permissions['{engine}']");
        for s in nr.allow.iter().chain(nr.ask.iter()).chain(nr.deny.iter()) {
            crate::config::validate_permission_string(&ctx, s)?;
        }
    }

    // Validate bundle-contributed memory entries with the same checks that
    // Config::validate() applies to top-level features.memory entries.
    if let Some(features) = &caps.features {
        for mem in &features.memory {
            if mem.when.is_empty() {
                anyhow::bail!(
                    "{context}: features.memory entry for '{}' has no 'when' tags — every memory entry must declare at least one activation tag",
                    mem.server_host
                );
            }
            if mem.listen_host.parse::<std::net::IpAddr>().is_err() {
                anyhow::bail!(
                    "{context}: features.memory entry for '{}': listen_host '{}' is not a valid \
                     IP address literal (hostnames not supported)",
                    mem.server_host,
                    mem.listen_host
                );
            }
            if let Some(n) = mem.wakeup_max_tokens
                && !crate::config::WAKEUP_MAX_TOKENS_RANGE.contains(&n)
            {
                anyhow::bail!(
                    "{context}: features.memory entry for '{}': wakeup_max_tokens ({n}) must be \
                     between {} and {} (icm's own MCP handler clamps to this range; llmenv \
                     fails fast instead of silently truncating)",
                    mem.server_host,
                    crate::config::WAKEUP_MAX_TOKENS_RANGE.start(),
                    crate::config::WAKEUP_MAX_TOKENS_RANGE.end()
                );
            }
        }
        for th in &features.throttle {
            if th.when.is_empty() {
                anyhow::bail!(
                    "{context}: features.throttle entry for '{}' has no 'when' tags — \
                     every throttle entry must declare at least one activation tag",
                    th.backend
                );
            }
            if th.backend.is_empty() {
                anyhow::bail!("{context}: features.throttle entry has an empty 'backend' field");
            }
        }
        for cm in &features.codebase_memory {
            if cm.when.is_empty() {
                anyhow::bail!(
                    "{context}: features.codebase_memory entry has no 'when' tags — \
                     every codebase_memory entry must declare at least one activation tag"
                );
            }
        }
    }

    Ok(Some(caps))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_none_and_a_text_file_is_read() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_bundle_text(&dir.path().join("AGENTS.md")).unwrap(),
            None
        );
        std::fs::write(dir.path().join("AGENTS.md"), "rules").unwrap();
        assert_eq!(
            read_bundle_text(&dir.path().join("AGENTS.md"))
                .unwrap()
                .as_deref(),
            Some("rules")
        );
    }

    #[test]
    fn a_link_to_a_text_file_is_followed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("shared.md"), "shared").unwrap();
        std::os::unix::fs::symlink(dir.path().join("shared.md"), dir.path().join("AGENTS.md"))
            .unwrap();
        assert_eq!(
            read_bundle_text(&dir.path().join("AGENTS.md"))
                .unwrap()
                .as_deref(),
            Some("shared")
        );
    }

    #[test]
    fn a_dangling_link_a_folder_and_bad_text_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("AGENTS.md");
        std::os::unix::fs::symlink(dir.path().join("moved.md"), &link).unwrap();
        let err = read_bundle_text(&link).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
        let folder = dir.path().join("bundle.yaml");
        std::fs::create_dir(&folder).unwrap();
        let err = read_bundle_text(&folder).unwrap_err();
        assert!(err.to_string().contains("not a regular file"), "{err}");
        let bad = dir.path().join("bad.md");
        std::fs::write(&bad, b"ok\xffno").unwrap();
        let err = read_bundle_text(&bad).unwrap_err();
        assert!(err.to_string().contains("offset 2"), "{err}");
    }

    proptest::proptest! {
        #[test]
        fn small_files_read_back_exactly_or_fail_as_bad_text(
            bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..512),
        ) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("AGENTS.md");
            std::fs::write(&path, &bytes).unwrap();
            match (read_bundle_text(&path), std::str::from_utf8(&bytes)) {
                (Ok(Some(text)), Ok(expected)) => proptest::prop_assert_eq!(text.as_str(), expected),
                (Err(e), Err(_)) => {
                    proptest::prop_assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
                }
                (got, want) => proptest::prop_assert!(false, "{got:?} vs {want:?}"),
            }
        }
    }

    #[test]
    fn a_fifo_fails_at_once_instead_of_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("AGENTS.md");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        let err = read_bundle_text(&fifo).unwrap_err();
        assert!(err.to_string().contains("not a regular file"), "{err}");
    }

    #[test]
    fn an_oversized_file_fails_with_its_size_rule() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("bundle.yaml");
        let size = usize::try_from(MAX_BUNDLE_FILE_BYTES).unwrap() + 1;
        std::fs::write(&big, vec![b'a'; size]).unwrap();
        let err = read_bundle_text(&big).unwrap_err();
        assert!(err.to_string().contains("more than"), "{err}");
        std::fs::write(&big, vec![b'a'; size - 1]).unwrap();
        assert!(
            read_bundle_text(&big).unwrap().is_some(),
            "the limit itself is allowed"
        );
    }
}
