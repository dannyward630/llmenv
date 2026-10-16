//! Version triples for the tools that llmenv checks, and the codebase-memory-mcp floor.

/// Oldest codebase-memory-mcp without the stability fixes that llmenv's SessionStart path depends
/// on: large indexes no longer corrupt the database, the startup settings race is gone, and Git
/// commands ignore hook-inherited `GIT_DIR` (upstream #2330, #2650, #2341).
const CBM_MIN_VERSION: (u64, u64, u64) = (0, 12, 0);

/// Parse `X.Y.Z` with an optional leading `v` and an optional `-suffix` or
/// `+suffix` on Z. `None` when the text does not have that shape.
///
/// A hand-written parse keeps a whole crate out of the tree for one comparison.
pub(super) fn parse_semver_triple(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.strip_prefix('v').unwrap_or(text);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = || {
        let part = parts.next()?;
        // `[0-9]{1,9}`: `char::is_numeric` would accept full-width digits.
        let is_digits = (1..=9).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit());
        if is_digits {
            part.parse::<u64>().ok()
        } else {
            None
        }
    };
    let triple = (next()?, next()?, next()?);
    parts.next().is_none().then_some(triple)
}

/// The warning text for a codebase-memory-mcp older than [`CBM_MIN_VERSION`],
/// or `None` when the version is at the floor, above it, or unparseable.
pub(super) fn cbm_floor_warning(version: &str) -> Option<String> {
    let parsed = parse_semver_triple(version)?;
    (parsed < CBM_MIN_VERSION).then(|| {
        format!(
            "codebase-memory-mcp {version} is older than 0.12.0. Older releases can corrupt a \
             large index and fail at startup on a settings race. The first index after \
             upgrading rebuilds each project once."
        )
    })
}
