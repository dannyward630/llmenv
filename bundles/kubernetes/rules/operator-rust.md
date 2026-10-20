---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
---

# Kubernetes Operator Rules (kube-rs)

Applies to Rust operators built on `kube-rs`. For language-agnostic Kubernetes manifest
conventions, see [`workloads.md`](workloads.md).

## Error Handling

SHOULD use `eyre::Result` with `color-eyre` for rich context: `eyre::eyre!()` to create, `.wrap_err()` to
add context as errors propagate, `.suggestion()` (from `color_eyre::Section`) for actionable user
guidance. At the kube-rs boundary, MUST convert to an error type implementing `std::error::Error`.

## Logging

MUST use `tracing` with structured fields and `#[instrument]`:

- `info!`, `debug!`, `warn!`, `error!` — MUST NOT use `println!`
- SHOULD use `#[instrument(skip(obj), fields(obj.name = %name))]` on reconcile functions
- SHOULD mark reconcile functions with `#[instrument]` to make reconciliation the root span — MUST NOT
  instrument the `Controller` setup function (that creates a single application-lifetime span)

## Reconciliation Invariants

- Reconciliation MUST be idempotent — triggered twice for the same object, it produces the same
  outcome.
- **Defensive reconciliation:** MUST assume you don't know why reconciliation started; any prior step may
  have failed. MUST check each operation independently rather than using all-or-nothing blocks:

  ```rust
  // WRONG: second operation never runs if the first fails partway
  if work_not_done {
      do_first_operation()?;
      do_second_operation()?;
  }

  // RIGHT: each operation independently checked and retryable
  if operation_1_incomplete() { perform_operation_1()?; }
  if operation_2_incomplete() { perform_operation_2()?; }
  ```

- **SHOULD prefer Server-Side Apply** over read-modify-write cycles — SSA is natively idempotent and
  eliminates most pre-check conditionals. Less if/else gating means fewer tests.
- MUST set `status.observedGeneration = metadata.generation` so clients detect stale status.
- MUST set status conditions on first visit, even as `Unknown`.
- MUST use `resourceVersion` for optimistic locking on status updates; MUST retry on 409 Conflict.
- MUST NOT copy values from a referenced resource into the referrer's spec (privilege escalation).
- SHOULD emit K8s events for user-visible state changes with stable UpperCamelCase reasons; SHOULD reuse the same
  reason/message for repeated events so Kubernetes can aggregate them.

## Garbage Collection

| Scenario | Mechanism |
|----------|-----------|
| K8s child object cleanup | Owner references (automatic) |
| External resource cleanup | Finalizers (programmatic) |
| Simple object deletion | Neither — `kubectl delete` suffices |

- **Owner references:** MUST set on all generated child objects via `Resource::controller_owner_ref()`.
  Enables automatic cleanup when the parent is deleted and lets `Controller::owns()` trigger
  reconciliation when children change.
- **Finalizers:** MUST use when an object needs programmatic cleanup of external resources (external API
  state, DNS records). The `finalizer()` helper splits reconciliation into `Event::Apply` (normal)
  and `Event::Cleanup` (deletion). Caveat: if the controller is down, deletes are delayed until it
  recovers — MUST keep finalizer cleanup fast and idempotent.

## Object Relations

- **`owns()`** — child objects managed by the controller; relies on `ownerReferences` for automatic
  GC. Changes trigger reconciliation of the owner.
- **`watches()`** — related objects without an ownership hierarchy; requires a mapper function that
  extracts a reference back to the root object from the watched resource.
- **`reconcile_on()`** — stream changes from external APIs as `ObjectRef` values to trigger
  reconciliation. MUST use finalizers for cleanup on deletion.

## Stream Configuration

- SHOULD use `metadata_watcher` as the default for owned/watched streams — returns only
  `TypeMeta`/`ObjectMeta`, reducing IO and memory when the full spec isn't needed.
- SHOULD apply **generation predicates** to filter status-only updates and prevent recursive reconcile
  triggers (reconciler updates status → triggers another reconcile).
- MUST continuously poll the controller's output stream — if nothing drives it, no work happens.

## Metrics

MUST expose three reconciliation metrics via a Prometheus-compatible endpoint:

1. **Reconciliation counter** — total attempts
2. **Failure counter** — errors, labeled by instance and error type
3. **Reconcile duration histogram** — time to complete

SHOULD use a `Drop`-based wrapper (e.g. `ReconcileMeasurer`) for automatic duration recording on scope
exit — guarantees measurement even when reconciliation errors out. Useful alert thresholds: >10%
error rate, zero reconciliations over a window, p90 latency >30s.

## Testing Strategy

SHOULD follow the test pyramid — invest most at the bottom:

1. **Unit tests** — business logic isolated from K8s. Separate IO from pure logic ("sans-IO").
2. **Unit tests with mocks** — `tower_test::mock::pair()` for a mocked `Client`; verify API call
   sequences without a cluster.
3. **Integration tests** — against a real cluster (`Client::try_default()`); k3d locally. Beware
   shared state and name collisions.
4. **E2E tests** — install the packaged release into a cluster. SHOULD use sparingly for smoke tests.

SSA reduces the need for tests — fewer conditional branches in the reconciler, fewer tests.

## Availability & Scaling

- **Single replica is the default** — controllers consume unsynchronized watch events; multiple
  replicas without coordination create duplicate work and races.
- **Leader election** SHOULD be used for HA: Kubernetes Leases as distributed locks (`kube-leader-election`,
  `kube-coordinate`, `kubert::lease`). One active replica, others standby.
- MUST handle SIGTERM gracefully during rollouts — drain the reconcile queue and let the lease expire
  before terminating.
- Scale in order: controller optimizations (cache/memoize, checkpoint progress on `.status`) →
  vertical scaling (tune `controller::Config` concurrency to CPU; the unlimited default can trip
  API server flow-control) → sharding (namespace/label-based partitioning).
