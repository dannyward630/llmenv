---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
---

# CRD Design Rules (kube-rs)

Applies to CustomResourceDefinitions defined in Rust (`kube` / `CustomResource` derive).

## Backwards Compatibility

- New fields MUST use `Option<T>` with `#[serde(default)]` — required fields break existing CRs.
- Renaming or removing fields breaks existing CRs — MUST add the new field alongside the old with a
  migration path.
- MUST NOT change field semantics. It silently corrupts state.
- `spec.selector.matchLabels` are immutable — MUST NOT derive selector labels from mutable values.
- After Helm/operator upgrades, MUST verify CRD instances retain all spec fields (3-way merge can drop
  newly-added fields).

## Field Merging & Config Composition

When composing a partial spec over defaults (CR over app-default, per-app over stack-default), MUST use
field-level merge helpers — not wholesale `unwrap_or(default)`, which lets a serde-defaulted field
(`""`, `0`, `Http`) clobber a real default. The footgun is the **argument direction**, so MUST make it
uniform and misuse-resistant:

- **MUST use one direction for every helper.** MUST pick `override.merge_with(&base) -> Merged`: the receiver
  (`self`) is the higher-priority value and wins per field; the argument is the fallback. MUST NOT ship
  two helpers with opposite receiver meaning (e.g. one `user.merge_with(&base)` and one
  `base.merge_with(&over)`) — a swapped call silently drops user input **and still type-checks**.
- **MUST name the parameter for its role** (`base` / `fallback`), and MUST NOT use `defaults` when the receiver is
  also a "defaults" value — that naming is what invites the swap.
- **Per field:** MUST keep `self`'s value unless it's the empty/zero/`None` sentinel, then take the
  fallback. Enums without a sentinel (e.g. `probe_type`) take `self` unconditionally — which is
  exactly why a swapped receiver corrupts them with no error.
- **MUST test the direction.** Every mergeable type MUST have a unit test asserting an explicit override
  value beats a *non-empty* default (not just that a missing field inherits). The "missing inherits"
  test alone passes even when the arguments are swapped.

## Newtype Patterns

The newtype rules are in [`rust.md`](rust.md) §Newtypes (rust-dev bundle).
For validated CRD string fields, SHOULD use `#[serde(try_from = "String")]`.

## Time-Based Configuration

SHOULD prefer a `Duration` newtype that accepts human-friendly strings (`5m`, `1h`, `1h30m`) as well as
bare seconds (`300`), stored internally as whole seconds. When that isn't possible (third-party CRD
constraints), MUST fall back to **seconds, never milliseconds** — Kubernetes Job deadlines and database
timeouts operate in seconds, and millisecond precision adds complexity without value for
operational settings. All defaults and documentation SHOULD use seconds or larger units.

## Enum String Representations

SHOULD prefer strum derives (`Display`, `AsRefStr`, `IntoStaticStr`) over manual `as_str()` or `Display`
impls. Serde and strum are independent — MUST keep `#[serde(rename = "...")]` and
`#[strum(serialize = "...")]` aligned when both are needed.

## CRD Generation

If the CRD YAML is generated from Rust source (e.g. `kube`'s `CustomResourceExt::crd()`), **MUST NOT
hand-edit the generated manifest** — edits are overwritten on rebuild. MUST regenerate whenever any CRD
source file changes, and after a rebase/merge that brings in CRD-affecting commits. MUST verify the
generated file matches source with a `git diff` against the base branch after rebasing.
