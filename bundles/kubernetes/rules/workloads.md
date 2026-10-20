---
paths:
  - "**/*.yaml"
  - "**/*.yml"
---

# Kubernetes Workload Rules

Language-agnostic conventions for any emitted Kubernetes manifests (Helm templates, KOTS,
operator-generated objects, raw YAML). For Rust kube-rs operator patterns, see
[`operator-rust.md`](operator-rust.md).

SHOULD use the `kubernetes` MCP server for cluster work. llmenv provides it on the `kubernetes` tag.

## Resource Emission Requirements

- Every container MUST set CPU and memory **requests and limits**.
- Every long-running container MUST have **readiness and liveness probes**.
- MUST NOT emit `:latest` image tags — MUST use explicit tags or digests.
- MUST set `terminationGracePeriodSeconds`; SHOULD use PreStop hooks when containers need drain time.
- Deployments MUST set `strategy.rollingUpdate` with explicit `maxSurge` and `maxUnavailable`.
- Multi-replica Deployments SHOULD set `topologySpreadConstraints` for zone awareness.

## Container Security

- MUST run as a non-root user.
- MUST set `allowPrivilegeEscalation: false`.
- MUST set `readOnlyRootFilesystem: true`.
- MUST drop all Linux capabilities (`drop: ["ALL"]`).
- SHOULD use distroless or Chainguard base images — SHOULD avoid ubuntu/debian/alpine (a shell is escalation
  surface). SHOULD use `kubectl debug` with ephemeral containers for debugging.

## RBAC

- MUST declare minimal permissions — only the verbs and resources actually used.
- SHOULD prefer namespaced `Role` + `RoleBinding` over cluster-wide `ClusterRole` when the workload
  operates in specific namespaces.
- MUST scope write access carefully — limit to non-delete verbs and specific `resourceNames` where
  possible.
- **MUST audit RBAC whenever** code introduces or modifies a Kubernetes API call, a subchart/dependency
  is added or upgraded, or CRD definitions change. Namespace-scoped resources MUST be in both
  `Role` and `ClusterRole` templates; cluster-scoped resources (namespaces, CRDs, nodes,
  storageclasses) MUST be only in the `ClusterRole`, and the code using them MUST degrade gracefully
  in namespace-only installs. New API groups/resources MUST have a comment explaining what they're
  for.

## Network Policies

MUST default-deny ingress and egress, and selectively permit:

- DNS resolution (UDP 53 to kube-dns)
- API server communication (TCP 443/6443)
- Observability egress (OTLP ports 4317/4318)
- Prometheus scraping ingress from the monitoring namespace

## Supply Chain

- SHOULD pin image digests where reproducibility matters.
- MUST run dependency/vulnerability scanning (`trivy`, `grype`) on images in CI.
- SHOULD embed an SBOM for downstream scanning.
- SHOULD automate dependency updates (Dependabot/Renovate) with a review cooldown.
