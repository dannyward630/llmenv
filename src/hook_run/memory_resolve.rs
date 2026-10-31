//! Memory-backend resolution for the active scopes: the merged memory entries, the endpoint
//! that wins, and the reasons when none does.

use super::*;

/// The memory and host entries that apply to the active scopes. They come from the top-level
/// config and the firing bundles, merged the way `build_manifest` merges them. The firing bundles
/// and their refs stay here, so endpoint classification can explain a miss.
pub(crate) struct MergedMemory<'a> {
    firing: Vec<&'a crate::config::Bundle>,
    bundle_refs: Vec<crate::merge::BundleRef>,
    pub(crate) memory: Vec<crate::config::Memory>,
    pub(crate) host: std::collections::BTreeMap<String, crate::config::HostEntry>,
}

/// Merge the memory and host entries for the active scopes. Endpoint resolution and
/// `llmenv doctor` both use it, so neither one misses a bundle-declared memory server.
///
/// # Errors
/// A firing bundle's `bundle.yaml` cannot be read or merged.
pub(crate) fn merged_memory<'a>(
    config: &'a crate::config::Config,
    config_dir: &std::path::Path,
    active: &crate::scope::ActiveScopes,
) -> anyhow::Result<MergedMemory<'a>> {
    let top_memory = config
        .features
        .as_ref()
        .map(|f| f.memory.as_slice())
        .unwrap_or_default();

    // Collect bundle-contributed memory and host entries. Bundle selection goes
    // through `cli::firing_bundles` — the same selector `build_manifest` uses —
    // so `disable_bundles` suppression can't drift between hook-run's live
    // resolution and the materialized manifest (#1125). The `tag_filter` must
    // stay `None`: it exists for the CLI's `--tag` flag, and narrowing endpoint
    // resolution by one tag would drop the memory backend for a live session.
    let firing = crate::cli::firing_bundles(&config.bundle, active, None);

    let bundle_refs = crate::cli::build_bundle_refs(config_dir, active, &firing);
    let (bundle_memory, bundle_host) = resolve_bundle_memory_host(config, &bundle_refs)?;

    let mut memory: Vec<crate::config::Memory> = top_memory
        .iter()
        .chain(bundle_memory.iter())
        .cloned()
        .collect();
    crate::util::dedup(&mut memory);

    // Merged host: bundle contributions first, top-level overwrites (same as build_manifest).
    let mut host = bundle_host;
    for (k, v) in &config.host {
        host.insert(k.clone(), v.clone());
    }
    Ok(MergedMemory {
        firing,
        bundle_refs,
        memory,
        host,
    })
}

/// Find the resolved memory backend's HTTP URL for the active tags, or the
/// reason none resolved.
///
/// Mirrors the `build_manifest` merge strategy: top-level config memory is
/// combined with bundle-contributed memory entries so a daemon declared only
/// in a `bundle.yaml` is reachable from lifecycle hooks.
///
/// `pub(crate)`: also called by `session_log::detached::run_record`, the
/// detached transcript-record child, which re-resolves the same MCP endpoint
/// independently rather than receiving it as a (process-list-visible) CLI arg.
///
/// # Errors
/// A bundle merge failure (#1132) or an unresolvable MCP/memory declaration.
pub(crate) fn memory_url(
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    active: &crate::scope::ActiveScopes,
) -> anyhow::Result<MemoryEndpoint> {
    let MergedMemory {
        firing,
        bundle_refs,
        memory: all_memory,
        host: all_host,
    } = merged_memory(config, config_dir, active)?;

    // Full `active.tags` (not `non_project_tags()`) on purpose: this resolves
    // the memory backend for the *live* hook-run session, which is legitimately
    // project-aware — unlike `build_manifest`'s static host-cache render, which
    // must exclude project-only tags (#696/#979). Do not "align" this with
    // build_manifest's host_tags; that would break recall in project scopes.
    let resolved = resolve_mcps(&config.mcp, &all_memory, &all_host, &active.tags)
        .map_err(|e| annotate_resolve_error(e, config, config_dir, active))?;
    let matched = resolved.into_iter().find_map(|m| match m.kind {
        ResolvedKind::Remote { url, .. } if m.name == MEMORY_MCP_NAME => Some((url, m.memory_hook)),
        _ => None,
    });
    Ok(match matched {
        Some((url, settings)) => MemoryEndpoint::Active {
            url,
            settings: settings.unwrap_or_else(default_memory_hook),
        },
        None => classify_missing_memory(
            config,
            config_dir,
            active,
            &firing,
            &bundle_refs,
            &all_memory,
        ),
    })
}

/// Explain why no memory endpoint resolved (#1131).
///
/// Only reached once resolution has already come up empty, so the extra
/// `bundle.yaml` reads it does to attribute a cause stay off the hot path that
/// every hook event takes.
///
/// `all_memory` is the same merged top-level + bundle-contributed list
/// `resolve_mcps` was called with. By construction every entry in it has a
/// `when` that doesn't intersect `active.tags`: an intersecting entry would
/// have either resolved (`MemoryEndpoint::Active`) or, if more than one
/// intersected, made `resolve_mcps` return `Err` before this function is ever
/// reached. So a non-empty `all_memory` here means "declared, but tag-inactive"
/// (#1140), not "declared and active."
///
/// Priority order (checked in this order because each is a strictly more
/// actionable cause than the next): `disable_bundles` suppression, then a
/// firing bundle `build_bundle_refs` couldn't load (a real misconfiguration —
/// its `features.memory`, if any, was never even read), then a declared but
/// tag-inactive entry (often intentional — e.g. a `when` scoped to a network
/// the user isn't on right now), then no bundles firing at all, then the
/// fully benign case of bundles that fired, loaded, and simply declare no
/// memory.
pub(super) fn classify_missing_memory(
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    active: &crate::scope::ActiveScopes,
    firing: &[&crate::config::Bundle],
    bundle_refs: &[crate::merge::BundleRef],
    all_memory: &[crate::config::Memory],
) -> MemoryEndpoint {
    let suppressed = suppressed_memory_bundles(config, config_dir, active);
    if !suppressed.is_empty() {
        return MemoryEndpoint::SuppressedByDisableBundles(suppressed);
    }
    let loaded: std::collections::HashSet<&str> =
        bundle_refs.iter().map(|r| r.name.as_str()).collect();
    let skipped_bundles: Vec<String> = firing
        .iter()
        .map(|b| b.name.as_str())
        .filter(|n| !loaded.contains(n))
        .map(str::to_owned)
        .collect();
    if !skipped_bundles.is_empty() {
        return MemoryEndpoint::NotDeclared { skipped_bundles };
    }
    if !all_memory.is_empty() {
        return MemoryEndpoint::TagInactive {
            server_hosts: all_memory
                .iter()
                .map(|m| m.server_host.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
        };
    }
    if firing.is_empty() {
        return MemoryEndpoint::NoBundlesFired;
    }
    MemoryEndpoint::NotDeclared {
        skipped_bundles: Vec::new(),
    }
}

/// Name `disable_bundles` in a resolution failure when that is what withdrew
/// the `host:` entry the memory block points at (#1131). Without it the user
/// sees a `server_host` they can read in their own `config.yaml` and nothing
/// connecting it to the bundle they turned off.
fn annotate_resolve_error(
    err: crate::mcp::resolve::ResolveError,
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    active: &crate::scope::ActiveScopes,
) -> anyhow::Error {
    if let crate::mcp::resolve::ResolveError::MemoryUnknownServerHost(host) = &err {
        let suppliers: Vec<String> = suppressed_bundle_capabilities(config, config_dir, active)
            .into_iter()
            .filter(|(_, caps)| caps.host.contains_key(host))
            .map(|(name, _)| name)
            .collect();
        if !suppliers.is_empty() {
            return anyhow::anyhow!(
                "failed to resolve MCP servers: {err} — it is declared in bundle(s) \
                 {}, which this project turns off via disable_bundles",
                suppliers.join(", ")
            );
        }
    }
    anyhow::anyhow!("failed to resolve MCP servers: {err}")
}

/// `bundle.yaml` capabilities of every bundle the active scopes suppress via
/// `disable_bundles` (#194) but that would otherwise have fired, in config
/// declaration order.
///
/// Diagnostic-only, read on the failure path so llmenv can say *why* no
/// endpoint resolved rather than only *that* none did. A suppressed bundle
/// whose own `bundle.yaml` is missing or unreadable contributes nothing here —
/// a failed explanation must not replace the failure being explained.
///
/// `pub(crate)`: also called by `cli::doctor`, whose orphaned-memory check must
/// see the same suppressed declarations this diagnostic does.
fn suppressed_bundle_capabilities(
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    active: &crate::scope::ActiveScopes,
) -> Vec<(String, crate::config::Capabilities)> {
    let disabled = crate::cli::marker_disabled_bundle_names(active);
    if disabled.is_empty() {
        return Vec::new();
    }
    let manually_enabled = crate::cli::marker_enabled_bundle_names(active);
    let would_fire: Vec<&crate::config::Bundle> = config
        .bundle
        .iter()
        .filter(|b| disabled.contains(&b.name))
        .filter(|b| crate::cli::tag_or_marker_selected(b, active, &manually_enabled))
        .collect();
    crate::cli::build_bundle_refs(config_dir, active, &would_fire)
        .into_iter()
        .filter_map(|r| {
            crate::merge::read_bundle_yaml(&r.path, &r.name)
                .inspect_err(|e| {
                    crate::agent_notice::record(format!(
                        "llmenv could not read bundle {} to check its suppressed capabilities: \
                         {e:#}. Fix: check the bundle's bundle.yaml.",
                        r.name
                    ));
                })
                .ok()
                .flatten()
                .map(|caps| (r.name, caps))
        })
        .collect()
}

/// Names of [`suppressed_bundle_capabilities`]' bundles that would supply a
/// tag-active `features.memory` entry if `disable_bundles` didn't suppress
/// them — the "would this disabled bundle have supplied memory" filter,
/// shared so `classify_missing_memory` and
/// `cli::doctor::memory_orphaned_by_disable_bundles` can't drift on it
/// (#1141). Tag-active, not merely present (#1140): a suppressed bundle whose
/// only `features.memory` entry is itself gated on an inactive tag wouldn't
/// have supplied memory even if re-enabled, so it must not be named as the
/// cause.
///
/// `pub(crate)`: called by `cli::doctor`, whose orphaned-memory check needs
/// the same filtered list this diagnostic does.
pub(crate) fn suppressed_memory_bundles(
    config: &crate::config::Config,
    config_dir: &std::path::Path,
    active: &crate::scope::ActiveScopes,
) -> Vec<String> {
    suppressed_bundle_capabilities(config, config_dir, active)
        .into_iter()
        .filter(|(_, caps)| {
            caps.features.as_ref().is_some_and(|f| {
                f.memory
                    .iter()
                    .any(|m| crate::mcp::resolve::memory_is_tag_active(m, &active.tags))
            })
        })
        .map(|(name, _)| name)
        .collect()
}
