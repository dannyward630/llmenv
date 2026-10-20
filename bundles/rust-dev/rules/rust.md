---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
---

# Rust Conventions

## Toolchain Configuration

When a project uses a pinned `rust-toolchain.toml`, MUST explicitly declare required components in the `components` array. This ensures rustup installs them without requiring a separate `rustup component add` step.

`rust-analyzer` MUST be in the `components` list for the LSP to work out of the box. Without it, rust-analyzer fails to initialize even when installed globally, because `rustup` prioritizes the pinned toolchain and does not auto-sync components.

```toml
[toolchain]
channel = "stable"
components = ["rust-analyzer", "rustfmt", "clippy"]
```

## Workspace Lint Policy

MUST enforce strict linting in the root `Cargo.toml`:

```toml
[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
panic = "deny"
panic_in_result_fn = "deny"
unimplemented = "deny"
allow_attributes = "deny"
dbg_macro = "deny"
todo = "deny"
await_holding_lock = "deny"
exit = "deny"
mem_forget = "deny"
# lib crates: tracing only (see Logging). Bin crates with a CLI may relax
# print_stdout to "allow" for user-facing output.
print_stdout = "deny"
print_stderr = "deny"
module_name_repetitions = "allow"
similar_names = "allow"

[workspace.lints.rust]
unsafe_code = "deny"
```

MUST use `?` and the appropriate error type for each crate. MUST use `#[expect(lint, reason = "...")]`
over `#[allow(lint)]` — it warns when the suppressed lint stops firing, preventing stale suppressions.
`allow_attributes` is denied to enforce this.

**unsafe:** SHOULD prefer `deny` over `forbid` for `unsafe_code` so a narrowly-scoped
`#[expect(unsafe_code, reason = "...")]` can override where genuinely required (FFI, platform
wrappers). MUST keep unsafe minimal and localized to a single crate. Every unsafe block MUST have a
`// SAFETY:` comment.

## Error Handling Strategy

Each layer SHOULD use its strategy from this table.

| Layer | Strategy |
| ----- | -------- |
| Libraries | `thiserror` enums with `#[from]`, typed variants. MUST add `# Errors` docs on public `Result` fns. |
| Applications / binaries | `anyhow` or `eyre` + `color-eyre` with `.wrap_err()` and `.suggestion()` |

## Async Boundaries

SHOULD keep async confined to binary/application crates that need a runtime (tokio reconcile loops,
IPC, network). Library crates SHOULD stay pure synchronous unless their domain is inherently
async — sync I/O that blocks by design (e.g. HID, blocking syscalls) SHOULD stay sync.

## Pre-Commit Hygiene

MUST run `cargo fmt` after every Rust file edit, before staging. Same for `shfmt` on shell scripts.
A formatting hook failure that bounces a commit wastes a full rebuild cycle.

## Build Cadence

`CARGO_INCREMENTAL=0` turns off incremental compilation. SHOULD use it sparingly.

The first build in a repo has no incremental cache to reuse. The flag costs no time there.
Every build after the first one pays the full cost again. A sequence of builds is slower with
the flag set. This covers a plan run, a fix-and-retry loop, and a review pass.

1. SHOULD set `CARGO_INCREMENTAL=0` for the first build only.
2. SHOULD leave the flag unset for each build after that.
3. MAY set the flag again only when a step dirties the working tree in a non-standard way.
   Examples: a script rewrites generated source; a step changes the toolchain.

## Test Conventions

- **SHOULD use `Arc::make_mut` to mutate `Arc`-wrapped test data** — not the clone+rewrap pattern
  (`(*data.field).clone()` / `data.field = Arc::new(…)`). `Arc::make_mut` is shorter, idiomatic,
  and avoids the paired clone/rewrap that drifts out of sync.

## Newtypes

SHOULD use newtypes to make invalid states unrepresentable. **"parse, don't validate"** — MUST validate in the
constructor; existence then guarantees validity, and downstream never re-validates.

**SHOULD use when:** domain values with constraints (ranges, indices, IDs), semantic disambiguation
(`(u8, u8)` silently accepts wrong order; `(ReportId, AxisValue)` cannot), units where mixing is a
silent bug.

**SHOULD NOT:** wrap unconstrained types with no confusion risk — that's just noise.

**Structure — MUST keep the inner field private:**
```rust
pub struct AxisValue(i16);

impl AxisValue {
    pub fn new(raw: i16) -> Result<Self, AxisError> {
        if (AXIS_MIN..=AXIS_MAX).contains(&raw) {
            Ok(Self(raw))
        } else {
            Err(AxisError::OutOfRange(raw))
        }
    }
}
```

**SHOULD derive traits generously** — downstream can't add them later due to the orphan rule:
- MUST derive: `Debug, Clone, PartialEq`
- SHOULD derive when the inner type allows: `Eq, PartialOrd, Ord, Hash, Copy`
- SHOULD NOT derive `Default` unless zero/empty is meaningful

**`TryFrom` MUST delegate to `new()` — MUST NOT duplicate validation:**
```rust
impl TryFrom<i16> for AxisValue {
    type Error = AxisError;
    fn try_from(raw: i16) -> Result<Self, Self::Error> { Self::new(raw) }
}
```

**Access:** SHOULD prefer `AsRef` over `Deref` for constrained types. MUST use explicit accessors:
```rust
impl AxisValue {
    pub fn into_inner(self) -> i16 { self.0 }
    pub fn as_inner(&self) -> i16 { self.0 }  // Copy types: return by value
}
```

**MUST NOT implement `Borrow<T>`** unless the newtype hashes and compares identically to the inner type —
otherwise it breaks `HashMap`/`HashSet` lookups.

## General Conventions

- All public types MUST derive or implement `Debug`
- MUST NOT use glob re-exports (`pub use foo::*`) — export items individually
- SHOULD avoid vague names (`Manager`, `Handler`, `Processor`, `Service`) when a domain name exists
- SHOULD use enums for state machines and effect/variant types, not bools or magic ints
- Lib crates: MUST use `tracing` (`error!`/`warn!`/`info!`/`debug!`) only. MUST NOT use `println!`/`eprintln!`
- Bin crates with a CLI: MAY use `println!` for user-facing output; SHOULD use `tracing` for diagnostics
- Hardcoded magic values (timeouts, retries, buffer sizes, ranges) MUST have a comment explaining
  *why that value*. Timing constants and retry counts are a code smell — SHOULD make them
  configurable. If a value must be fixed, MUST prefix it `DEFAULT_` and MUST explain why it can't be tuned.

For broader API design (naming cost model, trait derives, `Send`/`Sync`, builder/sealed patterns):
[`rust-api.md`](rust-api.md). For runtime security footguns the borrow checker won't catch:
[`rust-hardening.md`](rust-hardening.md).
