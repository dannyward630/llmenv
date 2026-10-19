//! The roots codebase-memory-mcp may index (#2406).
//!
//! codebase-memory-mcp 0.11.0 records allowed roots in `<cache dir>/allowed_roots`. Only the user
//! adds a root: with `codebase-memory-mcp allow-root <path>`, which the user types. llmenv never
//! runs that command and never edits the file (#2916). It reads the roots with `allow-root --list`,
//! reports what the config declares and the server lacks, and names the command for the user.
//! Design: docs/design/issue-2406-cbm-allowed-roots.md

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::config::{CodebaseMemory, Config};
use crate::process::run_with_timeout;

mod declared;
pub(crate) use declared::{configured_roots, root_problems};

/// How long one `codebase-memory-mcp allow-root --list` call may take.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// The inputs that the roots of a session need. The defaults are not among them: a folder that
/// llmenv creates or uses is an allowed root only when the config declares it (#2916).
#[derive(Debug, Clone)]
pub(crate) struct RootBases {
    pub project_root: PathBuf,
    pub home: Option<PathBuf>,
}

impl RootBases {
    /// The session folder and the home folder of this process.
    pub(crate) fn for_project(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            home: std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(PathBuf::from),
        }
    }
}

/// The cache folder codebase-memory-mcp serves: `index_path`, or its own default.
///
/// # Errors
/// No `index_path` is set and `HOME` is unknown. A literal `~` would be
/// read as a directory name, so the caller must set `index_path` instead.
fn cache_dir(cm: &CodebaseMemory, home: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(index_path) = &cm.index_path {
        return Ok(PathBuf::from(crate::paths::expand_tilde(index_path)?));
    }
    let home = home.ok_or_else(|| {
        anyhow::anyhow!(
            "cannot locate the codebase-memory cache: HOME is not set. \
             Set features.codebase_memory index_path to a directory."
        )
    })?;
    Ok(home.join(".cache/codebase-memory-mcp"))
}

/// A `codebase-memory-mcp` command with the environment of the server llmenv launches.
///
/// # Errors
/// `index_path` cannot be expanded, for example a leading `~` with `HOME` unset.
fn cbm_command(cm: &CodebaseMemory, args: &[&str]) -> anyhow::Result<Command> {
    let mut cmd = Command::new("codebase-memory-mcp");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(index_path) = &cm.index_path {
        cmd.env("CBM_CACHE_DIR", crate::paths::expand_tilde(index_path)?);
    }
    Ok(cmd)
}

/// The roots in the output of `allow-root --list`.
/// The roots in the output of `allow-root --list`. A line that is neither a root nor a known
/// header or empty notice means the format changed. The roots are then unknown, so the read
/// fails rather than reading as an empty, unconfined record (#2916).
///
/// # Errors
/// A line that llmenv does not recognize.
fn parse_roots(listing: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for line in listing.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with('/') {
            roots.push(PathBuf::from(line));
        } else if !line.starts_with("allowed roots") && !line.starts_with("no allowed roots") {
            anyhow::bail!("unrecognized line in allow-root --list output: {line:?}");
        }
    }
    Ok(roots)
}

/// The roots that codebase-memory-mcp has recorded for the cache folder of `cm`.
///
/// # Errors
/// The command cannot run, times out, or exits non-zero.
fn list_roots(cm: &CodebaseMemory) -> anyhow::Result<Vec<PathBuf>> {
    let output = run_with_timeout(cbm_command(cm, &["allow-root", "--list"])?, COMMAND_TIMEOUT)
        .map_err(|e| {
            anyhow::anyhow!(
                "codebase-memory-mcp allow-root --list: {e:#}. Is codebase-memory-mcp on PATH?"
            )
        })?;
    anyhow::ensure!(
        output.status.success(),
        "codebase-memory-mcp allow-root --list exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    parse_roots(&String::from_utf8_lossy(&output.stdout))
}

/// What the server allows, compared with what the config declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootsReport {
    /// The roots the server has recorded.
    pub listed: Vec<PathBuf>,
    /// The declared roots that exist and that no recorded root contains, with the reason.
    pub missing: Vec<(PathBuf, String)>,
    /// The recorded roots that no declared root contains. Earlier llmenv versions recorded them.
    pub undeclared: Vec<PathBuf>,
    pub cache_dir: PathBuf,
}

/// The canonical form of `path`, or `None` when it does not resolve.
fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Whether `path` is one of `roots` or sits under one. Both sides resolve links first, so a
/// link into a recorded folder counts (#2406).
fn is_within_any(path: &Path, roots: &[PathBuf]) -> bool {
    let Some(path) = canonical(path) else {
        return false;
    };
    roots
        .iter()
        .filter_map(|root| canonical(root))
        .any(|root| path.starts_with(root))
}

/// The declared roots that the server does not cover. An empty record means the server is
/// unconfined, so nothing is missing (#2916). A root that does not exist cannot be recorded, so
/// it is left out.
fn missing_roots(wanted: &[PathBuf], listed: &[PathBuf]) -> Vec<PathBuf> {
    if listed.is_empty() {
        return Vec::new();
    }
    wanted
        .iter()
        .filter(|root| is_folder(root) && !is_within_any(root, listed))
        .cloned()
        .collect()
}

/// Whether `path` is a folder. A stat error other than "not found" is logged, because it
/// would otherwise drop a declared root from the report without a word (#2916).
fn is_folder(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_dir(),
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("cannot read the declared root {}: {e}", path.display());
            }
            false
        }
    }
}

/// The recorded roots that no declared root covers. A recorded root that sits under a declared
/// one is covered, so it is not reported.
fn undeclared_roots(listed: &[PathBuf], wanted: &[PathBuf]) -> Vec<PathBuf> {
    listed
        .iter()
        .filter(|root| {
            if canonical(root).is_none() {
                tracing::warn!(
                    "recorded root {} does not resolve, so it is not reported",
                    root.display()
                );
                return false;
            }
            !is_within_any(root, wanted)
        })
        .cloned()
        .collect()
}

/// Read-only: compare the declared roots with what the server has recorded. llmenv runs no
/// `allow-root` here (#2916).
///
/// # Errors
/// The list cannot be read (see [`list_roots`]).
pub(crate) fn check_roots(
    cm: &CodebaseMemory,
    wanted: &[PathBuf],
    home: Option<&Path>,
) -> anyhow::Result<RootsReport> {
    let listed = list_roots(cm)?;
    let missing = missing_roots(wanted, &listed)
        .into_iter()
        .map(|root| (root, "not in the allowed roots".to_string()))
        .collect();
    Ok(RootsReport {
        undeclared: undeclared_roots(&listed, wanted),
        listed,
        missing,
        cache_dir: cache_dir(cm, home)?,
    })
}

/// The command that the user runs to record `root` with the server of `cache_dir`.
fn allow_root_command(cache_dir: &Path, root: &Path) -> String {
    format!(
        "CBM_CACHE_DIR={} codebase-memory-mcp allow-root {}",
        crate::util::shell_word(cache_dir),
        crate::util::shell_word(root)
    )
}

/// The text that tells the user what `report` found. `warn` is true when a declared root is
/// missing. The user runs the commands in the text; llmenv never does (#2916).
pub(crate) fn describe(report: &RootsReport) -> (bool, String) {
    let list = |roots: &[PathBuf]| {
        roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let undeclared = if report.undeclared.is_empty() {
        String::new()
    } else {
        format!(
            " The server also holds roots that the config does not declare: {}. Earlier llmenv \
             versions recorded them. Ask the user to clear them: to return to unconfined \
             indexing, the user deletes {}; to keep some roots, the user edits that file. Do not \
             delete or edit it yourself.",
            list(&report.undeclared),
            report.cache_dir.join("allowed_roots").display()
        )
    };
    if report.missing.is_empty() {
        return (
            false,
            format!(
                "codebase-memory: allowed roots {} (cache {}).{undeclared}",
                list(&report.listed),
                report.cache_dir.display()
            ),
        );
    }
    let missing: Vec<String> = report
        .missing
        .iter()
        .map(|(root, why)| format!("{} ({why})", root.display()))
        .collect();
    let commands: Vec<String> = report
        .missing
        .iter()
        .map(|(root, _)| allow_root_command(&report.cache_dir, root))
        .collect();
    (
        true,
        format!(
            "codebase-memory cannot index {}. Ask the user to run: {}. llmenv never runs \
             allow-root itself.{undeclared}",
            missing.join(", "),
            commands.join("; ")
        ),
    )
}

/// The `repo_path` of an `index_repository` call that is outside the allowed roots, as the
/// reason to deny it. `None` when it is inside one. Both sides must be canonical, so a link into
/// a root passes and a `..` escape does not (#2406).
fn path_outside_roots(repo_path: &Path, roots: &[PathBuf]) -> Option<String> {
    if roots.iter().any(|root| repo_path.starts_with(root)) {
        return None;
    }
    let list = roots
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "{} is outside the codebase-memory allowed roots ({list}). Ask the user to record it: \
         `codebase-memory-mcp allow-root <path>`, with CBM_CACHE_DIR set to the entry's \
         index_path when one is set. Do not run allow-root yourself.",
        repo_path.display()
    ))
}

/// Read the roots at `SessionStart`. Returns the text to show the user when a declared root is
/// missing or the server cannot be asked, and `None` when all is well (#2406). It only reads: a
/// session never records a root (#2916).
pub(crate) fn session_start_notice(cm: &CodebaseMemory, project_root: &Path) -> Option<String> {
    let bases = RootBases::for_project(project_root);
    let wanted = configured_roots(cm, &bases);
    let problems = root_problems(cm, &bases);
    match check_roots(cm, &wanted, bases.home.as_deref()) {
        Ok(report) => {
            let (warn, text) = describe(&report);
            let undeclared = !report.undeclared.is_empty();
            let text = [(warn || undeclared).then_some(text)]
                .into_iter()
                .flatten()
                .chain(problems)
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then(|| format!("{text}\n"))
        }
        Err(e) => {
            tracing::error!("codebase-memory allowed roots could not be read: {e:#}");
            Some(format!(
                "codebase-memory: cannot read the allowed roots: {e:#}. Run `llmenv doctor`.\n"
            ))
        }
    }
}

/// The reason to deny an `index_repository` call, or `None` to let it pass (#2406). The allowed
/// set is what the server records, so a root that the user recorded counts.
pub(crate) fn guard_decision(repo_path: &str, config: &Config) -> Option<String> {
    let entries = config
        .features
        .as_ref()
        .map(|f| f.codebase_memory.as_slice())?;
    if entries.is_empty() {
        return None;
    }
    let mut listed = Vec::new();
    let mut unreadable = Vec::new();
    for cm in entries {
        match list_roots(cm) {
            Ok(roots) => listed.extend(roots),
            Err(e) => unreadable.push(format!("{e:#}")),
        }
    }
    decide_with_reads(repo_path, listed, &unreadable)
}

/// [`guard_decision`] given the listed roots and the failed reads. An empty record with a failed
/// read is not unconfined: the read may have failed, so the guard denies (#2916).
fn decide_with_reads(
    repo_path: &str,
    listed: Vec<PathBuf>,
    unreadable: &[String],
) -> Option<String> {
    if listed.is_empty() && !unreadable.is_empty() {
        return Some(format!(
            "{repo_path} cannot be checked against the codebase-memory allowed roots, because \
             they could not be read: {}. Ask the user to run `llmenv doctor`.",
            unreadable.join("; ")
        ));
    }
    decide(repo_path, listed).map(|reason| with_unreadable_note(reason, unreadable))
}

/// `reason` with the failures to read the server's recorded roots, so a deny that came from a
/// failed read does not look like a root the user forgot to add.
fn with_unreadable_note(reason: String, unreadable: &[String]) -> String {
    if unreadable.is_empty() {
        return reason;
    }
    format!(
        "{reason} The roots that the server recorded could not be read: {}.",
        unreadable.join("; ")
    )
}

/// [`guard_decision`] with the listed roots given. An empty record means the server is
/// unconfined, so the server decides and the guard passes (#2916). A `repo_path` that does not
/// exist is denied: the server cannot index it, and no canonical path exists to compare.
fn decide(repo_path: &str, listed: Vec<PathBuf>) -> Option<String> {
    if listed.is_empty() {
        return None;
    }
    // A relative path resolves against the hook's folder, but the server resolves it against its
    // own, so the check would judge a different folder than the one indexed (#2916).
    if !Path::new(repo_path).is_absolute() {
        return Some(format!(
            "{repo_path} is not an absolute path, so it cannot be checked against the \
             codebase-memory allowed roots. Pass the absolute path of the repository."
        ));
    }
    let roots: Vec<PathBuf> = listed.iter().filter_map(|root| canonical(root)).collect();
    if roots.is_empty() {
        return Some(format!(
            "{repo_path} cannot be checked: none of the {} recorded roots resolves to a folder. \
             Ask the user to run `llmenv doctor`.",
            listed.len()
        ));
    }
    match std::fs::canonicalize(repo_path) {
        Ok(canonical) => path_outside_roots(&canonical, &roots),
        Err(e) => Some(format!(
            "{repo_path} cannot be indexed: it does not resolve to a folder ({e}). Pass the \
             path of an existing repository."
        )),
    }
}

#[cfg(test)]
mod set_props;

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn cm(index_path: Option<&str>) -> CodebaseMemory {
        CodebaseMemory {
            when: vec!["p".into()],
            index_path: index_path.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn the_listing_is_read_after_its_header() {
        let listing = "allowed roots:\n/Users/u/git\n/Users/u/.cache/nbl-diag/repos\n";
        assert_eq!(
            parse_roots(listing).unwrap(),
            [
                PathBuf::from("/Users/u/git"),
                PathBuf::from("/Users/u/.cache/nbl-diag/repos")
            ]
        );
        assert!(
            parse_roots("no allowed roots recorded \n")
                .unwrap()
                .is_empty()
        );
        assert!(parse_roots("").unwrap().is_empty());
    }

    #[test]
    fn a_listing_in_a_new_format_is_an_error_not_an_empty_record() {
        // An empty record means unconfined, so a format change must not read as one (#2916).
        assert!(parse_roots("- /Users/u/git\n").is_err());
        assert!(parse_roots("allowed roots:\n  /Users/u/git\nsomething new\n").is_err());
    }

    #[test]
    fn only_existing_unrecorded_roots_are_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (have, lack, gone) = (
            dir.path().join("have"),
            dir.path().join("lack"),
            dir.path().join("gone"),
        );
        std::fs::create_dir(&have).unwrap();
        std::fs::create_dir(&lack).unwrap();
        let wanted = vec![have.clone(), lack.clone(), gone];
        assert_eq!(missing_roots(&wanted, std::slice::from_ref(&have)), [lack]);
    }

    #[test]
    fn an_empty_record_is_unconfined_so_nothing_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let lack = dir.path().join("lack");
        std::fs::create_dir(&lack).unwrap();
        assert!(missing_roots(&[lack], &[]).is_empty());
    }

    #[test]
    fn a_folder_under_a_recorded_root_is_covered() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("parent/child");
        std::fs::create_dir_all(&child).unwrap();
        let parent = dir.path().join("parent");
        assert!(missing_roots(std::slice::from_ref(&child), &[parent]).is_empty());
    }

    #[test]
    fn a_recorded_root_counts_through_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(missing_roots(&[link], &[real]).is_empty());
    }

    #[test]
    fn a_recorded_root_that_no_declared_root_covers_is_undeclared() {
        let dir = tempfile::tempdir().unwrap();
        let declared = dir.path().join("declared");
        let wider = dir.path().join("wider");
        std::fs::create_dir_all(declared.join("sub")).unwrap();
        std::fs::create_dir(&wider).unwrap();
        let undeclared = undeclared_roots(
            &[declared.clone(), wider.clone()],
            std::slice::from_ref(&declared),
        );
        assert_eq!(undeclared, [wider]);
        assert!(undeclared_roots(&[declared.join("sub")], &[declared]).is_empty());
    }

    #[test]
    fn the_description_lists_the_roots_or_the_missing_ones_with_the_fix() {
        let ok = RootsReport {
            listed: vec!["/a".into(), "/b".into()],
            missing: vec![],
            undeclared: vec![],
            cache_dir: "/cache".into(),
        };
        assert_eq!(
            describe(&ok),
            (
                false,
                "codebase-memory: allowed roots /a, /b (cache /cache).".to_string()
            )
        );
        let bad = RootsReport {
            missing: vec![("/c".into(), "refused".to_string())],
            ..ok
        };
        let (warn, text) = describe(&bad);
        assert!(warn);
        assert!(text.contains("/c (refused)"), "{text}");
        assert!(
            text.contains("CBM_CACHE_DIR=/cache codebase-memory-mcp allow-root /c"),
            "{text}"
        );
        assert!(text.contains("llmenv never runs"), "{text}");
    }

    #[test]
    fn the_description_names_the_undeclared_roots_and_the_file_to_clear() {
        let report = RootsReport {
            listed: vec!["/git".into()],
            missing: vec![],
            undeclared: vec!["/git".into()],
            cache_dir: "/cache".into(),
        };
        let (warn, text) = describe(&report);
        assert!(!warn);
        assert!(text.contains("/git"), "{text}");
        assert!(text.contains("/cache/allowed_roots"), "{text}");
    }

    #[test]
    fn a_path_with_spaces_is_quoted_for_the_shell() {
        assert_eq!(
            crate::util::shell_word(Path::new("/plain/path")),
            "/plain/path"
        );
        assert_eq!(crate::util::shell_word(Path::new("/a b")), "'/a b'");
        assert_eq!(crate::util::shell_word(Path::new("/it's")), "'/it'\\''s'");
        assert_eq!(crate::util::shell_word(Path::new("")), "''");
    }

    #[test]
    fn a_failed_read_with_an_empty_record_denies_the_call() {
        let reason = decide_with_reads("/usr", vec![], &["timed out".to_string()]).unwrap();
        assert!(reason.contains("could not be read"), "{reason}");
        assert!(reason.contains("timed out"), "{reason}");
        assert_eq!(decide_with_reads("/usr", vec![], &[]), None);
    }

    #[test]
    fn a_relative_repo_path_is_denied_not_resolved_against_the_hook_folder() {
        let listed = vec![std::env::current_dir().unwrap()];
        let reason = decide(".", listed).unwrap();
        assert!(reason.contains("not an absolute path"), "{reason}");
    }

    #[test]
    fn a_record_that_resolves_to_nothing_denies_the_call() {
        let reason = decide("/usr", vec![PathBuf::from("/no/such/root/2916")]).unwrap();
        assert!(reason.contains("none of the 1 recorded roots"), "{reason}");
    }

    #[test]
    fn the_cache_dir_is_the_index_path_or_the_default() {
        let mut c = cm(None);
        assert_eq!(
            cache_dir(&c, Some(Path::new("/home/u"))).unwrap(),
            Path::new("/home/u/.cache/codebase-memory-mcp")
        );
        c.index_path = Some("/idx".into());
        assert_eq!(cache_dir(&c, None).unwrap(), Path::new("/idx"));
    }

    #[test]
    fn the_cache_dir_without_home_or_index_path_is_an_error() {
        let err = cache_dir(&cm(None), None).unwrap_err();
        assert!(
            err.to_string().contains("index_path"),
            "error must name the fix: {err}"
        );
    }

    #[test]
    fn the_cache_dir_expands_a_leading_tilde_in_index_path() {
        let dir = cache_dir(&cm(Some("~/idx")), None).ok();
        assert!(
            dir.as_deref().is_some_and(Path::is_absolute),
            "a relative '~' directory would be a wrong cache: {dir:?}"
        );
    }

    #[test]
    fn the_command_gets_the_cache_dir_of_the_server() {
        let env_of = |c: &CodebaseMemory| {
            cbm_command(c, &["allow-root", "--list"])
                .ok()?
                .get_envs()
                .find(|(k, _)| *k == std::ffi::OsStr::new("CBM_CACHE_DIR"))
                .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        assert_eq!(env_of(&cm(None)), None);
        assert_eq!(env_of(&cm(Some("/idx"))).as_deref(), Some("/idx"));
    }

    #[test]
    fn a_path_is_inside_a_root_or_the_reason_names_the_fix() {
        let roots = vec![PathBuf::from("/work/proj"), PathBuf::from("/srv/x")];
        assert_eq!(path_outside_roots(Path::new("/work/proj"), &roots), None);
        assert_eq!(
            path_outside_roots(Path::new("/work/proj/sub/dir"), &roots),
            None
        );
        let reason = path_outside_roots(Path::new("/work/projx"), &roots).unwrap();
        assert!(
            reason.contains("/work/projx") && reason.contains("/work/proj, /srv/x"),
            "{reason}"
        );
        assert!(
            reason.contains("allow-root") && reason.contains("Do not run allow-root yourself"),
            "{reason}"
        );
        assert!(path_outside_roots(Path::new("/tmp/elsewhere"), &roots).is_some());
        assert!(path_outside_roots(Path::new("/tmp"), &[]).is_some());
    }

    #[test]
    fn an_empty_record_lets_every_path_through_the_guard() {
        assert_eq!(decide("/usr", vec![]), None);
    }

    #[test]
    fn the_guard_passes_a_repo_in_a_recorded_root_and_denies_others() {
        let base = tempfile::tempdir().unwrap();
        let inside = base.path().join("inside");
        let repo = inside.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let outside = base.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let listed = vec![inside];
        assert_eq!(decide(repo.to_str().unwrap(), listed.clone()), None);
        let reason = decide(outside.to_str().unwrap(), listed).unwrap();
        assert!(reason.contains("allow-root"), "{reason}");
    }

    #[test]
    fn the_guard_denies_a_path_that_does_not_exist_and_a_dot_dot_escape() {
        let base = tempfile::tempdir().unwrap();
        let inside = base.path().join("inside");
        std::fs::create_dir(&inside).unwrap();
        let listed = vec![inside.clone()];
        let missing = inside.join("gone");
        let reason = decide(missing.to_str().unwrap(), listed.clone()).unwrap();
        assert!(reason.contains("does not resolve"), "{reason}");
        let escape = inside.join("..");
        assert!(decide(escape.to_str().unwrap(), listed).is_some());
    }

    #[test]
    fn a_link_out_of_a_root_is_denied_and_a_link_into_one_passes() {
        let base = tempfile::tempdir().unwrap();
        let inside = base.path().join("inside");
        let outside = base.path().join("outside");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let out_link = inside.join("out");
        std::os::unix::fs::symlink(&outside, &out_link).unwrap();
        let in_link = outside.join("in");
        std::os::unix::fs::symlink(&inside, &in_link).unwrap();
        let listed = vec![inside];
        assert!(decide(out_link.to_str().unwrap(), listed.clone()).is_some());
        assert_eq!(decide(in_link.to_str().unwrap(), listed), None);
    }

    #[test]
    fn the_guard_does_not_apply_without_a_codebase_memory_entry() {
        assert_eq!(guard_decision("/anywhere", &Config::default()), None);
    }

    proptest! {
        #[test]
        fn every_parsed_root_is_an_absolute_line_of_the_input(listing in "(\\PC{0,30}\n){0,6}") {
            let Ok(roots) = parse_roots(&listing) else {
                return Ok(());
            };
            for root in roots {
                prop_assert!(root.is_absolute());
                prop_assert!(listing.lines().any(|l| l.trim() == root.to_string_lossy()));
            }
        }

        #[test]
        fn a_shell_word_is_never_empty(path in "\\PC{0,40}") {
            prop_assert!(!crate::util::shell_word(Path::new(&path)).is_empty());
        }

        #[test]
        fn a_path_under_any_root_is_inside_and_a_path_under_none_is_outside(
            roots in prop::collection::vec("/[a-z]{1,6}(/[a-z]{1,6}){0,2}", 1..4),
            pick in 0usize..4,
            tail in "(/[a-z]{1,6}){0,3}",
            other in "/zz[0-9]{1,4}(/[a-z]{1,6}){0,2}",
        ) {
            let roots: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();
            let inside = PathBuf::from(format!("{}{tail}", roots[pick % roots.len()].display()));
            prop_assert_eq!(path_outside_roots(&inside, &roots), None);
            prop_assert!(path_outside_roots(Path::new(&other), &roots).is_some());
        }
    }
}
