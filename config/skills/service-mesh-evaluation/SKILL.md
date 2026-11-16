---
name: service-mesh-evaluation
description: "Decides whether a service mesh (sidecar proxies handling mTLS, retries, traffic shifting, and observability for service-to-service traffic) is worth its operational cost for a given system, and if so, which capabilities to actually turn on. Use when service-to-service traffic concerns are being reimplemented per-language/per-service, or when someone proposes adopting Istio/Linkerd and the team needs a grounded yes/no."
---

# Service Mesh Evaluation

Decide, with an honest accounting of operational cost, whether a service mesh's sidecar-proxy layer for east-west (service-to-service) traffic is solving a real problem your team has today — not whether it's an interesting piece of infrastructure to run. A mesh is a significant operational commitment (control plane, sidecar upgrades, added latency, new failure modes) that only pays for itself past a certain service count and cross-cutting-traffic-concern burden.

## When to use this

- More than a handful of services are each independently implementing retries, timeouts, circuit breaking, or mTLS, and the implementations are inconsistent across languages/teams (see `circuit-breaker-pattern-design` for the pattern itself, independent of whether a mesh implements it).
- Security requirements mandate mTLS between all internal services and doing this per-service (cert issuance, rotation, verification) in application code is unsustainable.
- The team needs fine-grained traffic control — canary releases, traffic shifting by percentage, request-level routing rules — across many services, and today that logic is hand-rolled per service.
- Observability into service-to-service call latency, error rates, and retries is inconsistent because each service instruments differently, and a uniform east-west traffic view is needed.
- Someone has proposed adopting Istio, Linkerd, or a cloud-managed mesh, and leadership needs a grounded evaluation of whether the operational cost is justified given current service count and team size — not a reflexive yes or no.
- A platform team is being asked to support multiple languages/runtimes and wants to move cross-cutting network concerns out of per-language libraries into infrastructure that works identically regardless of language.

## Workflow

1. **Count real east-west traffic pain points before evaluating any product.** List the concrete symptoms today: inconsistent retry logic causing cascading failures, no uniform mTLS, canary releases done manually and error-prone, or fragmented latency visibility. If none of these are actual current pain, a mesh is solving a hypothetical problem — defer.
2. **Establish the service-count and team-size threshold honestly.** A mesh's control-plane and sidecar operational overhead (upgrades, resource cost per pod, debugging an extra network hop) is rarely justified below roughly 10-15 services or without a dedicated platform team to own it — a 5-service system with one team is almost always better served by in-app libraries (see `circuit-breaker-pattern-design`) or a lighter API-gateway-level solution.
3. **Separate the specific capabilities a mesh bundles and evaluate each on its own merit**: mTLS + identity, retries/timeouts/circuit-breaking, traffic shifting/canary routing, and telemetry (latency, error rate, trace propagation). A team might need only mTLS (solvable with a lighter cert-management tool) without needing full traffic-shifting capability — don't adopt the whole bundle to get one capability.
4. **Weigh sidecar-proxy cost explicitly**: every request now takes an extra network hop through the local proxy (typically single-digit milliseconds added latency), every pod carries extra CPU/memory overhead for the sidecar, and every deploy now has a sidecar-injection dependency that can itself fail or need rollback. Quantify this against current latency budgets before assuming it's negligible.
5. **Evaluate control-plane operational burden as its own line item.** Someone has to run and upgrade the mesh control plane (Istio's istiod, Linkerd's control plane), understand its failure modes, and debug mesh-specific issues (a sidecar injection failure, a misconfigured `VirtualService`) that didn't exist before — this needs a named owning team, not "we'll figure it out."
6. **Compare mesh-provided mTLS against alternatives** — a simpler mutual-TLS setup via cloud load balancer/gateway features, or a lighter cert-management sidecar, may deliver the security requirement without the full traffic-management surface area of a mesh.
7. **If proceeding, choose the mesh architecture deliberately**: sidecar-based (Istio, Linkerd — per-pod proxy, mature, higher resource cost) vs. sidecar-less/ambient (newer approaches reducing per-pod overhead, less mature tooling) — pick based on how much resource overhead per pod is tolerable and how mature the operational tooling needs to be for the team's risk appetite.
8. **Roll out incrementally, not to every service at once.** Start with mesh sidecar injection on a small, non-critical service group, validate the added latency and failure modes in production-like conditions, and expand namespace by namespace — a big-bang mesh rollout across every service is a common source of mass outages.
9. **Define what stays out of the mesh's responsibility.** Business-level retries with domain-aware idempotency, application-level circuit breaking on business-meaningful failure conditions, and north-south client-facing concerns (see `api-gateway-design`) generally don't belong in the mesh — the mesh handles transport-level east-west concerns, not business logic.
10. **Plan for mesh version upgrades and CVE response** as an ongoing operational commitment, not a one-time adoption cost — mesh control planes and sidecars need regular patching, and an outdated mesh version is itself a security liability.
11. **Set explicit success metrics before rollout** (e.g., "p99 inter-service latency added by sidecar stays under 5ms," "mTLS coverage reaches 100% of internal traffic within one quarter") so the mesh's value can be measured rather than assumed.

## Example

Scenario: `Meridian Health`, a healthcare platform with 22 microservices across 4 teams, currently has inconsistent retry/timeout behavior (some services retry 3 times with no backoff, others don't retry at all, causing occasional retry storms during dependency degradation), no mTLS between internal services (a compliance finding from a recent security audit), and manual canary releases done via load-balancer weight changes that are error-prone and hard to roll back quickly.

Evaluation:

| Capability needed | Current state | Mesh solves it? | Alternative considered |
|---|---|---|---|
| mTLS between all services | None; compliance finding open | Yes — automatic mTLS with identity-based policy | Manual per-service cert management — rejected: 22 services × 4 teams means cert rotation would be inconsistently implemented, recreating the same audit finding |
| Consistent retries/circuit-breaking | Ad hoc per service, inconsistent | Yes — uniform retry/timeout policy at the proxy layer | Shared retry library per language — rejected: team uses 3 languages (Go, Java, Python), a shared library needs 3x maintenance and still drifts |
| Canary/traffic shifting | Manual LB weight changes | Yes — declarative percentage-based traffic splitting | Feature flags at the app layer — kept as a complement for business-logic-level flags, but traffic-level canary is a better fit for the mesh |
| Uniform east-west telemetry | Fragmented, per-service instrumentation | Yes — automatic latency/error telemetry per call | — |

Decision: adopt a service mesh (Linkerd, chosen over Istio for lower resource overhead and simpler operational model given the team has no dedicated platform engineer yet). Rollout plan: inject sidecars into the lowest-risk namespace (internal reporting services, 3 services) first, measure added p99 latency (came in at 2.8ms, within the 5ms budget set upfront), then expand to compliance-critical services (where the mTLS requirement matters most) in month 2, and the remaining services by month 4.

Rejected alternative: full Istio adoption across all 22 services in one rollout. Rejected because the team has no dedicated platform engineer to own the control plane yet, and a big-bang rollout would make it impossible to isolate whether an incident was caused by the mesh or by an unrelated service change.

## Common pitfalls

- **Adopting a mesh because it's popular, not because of a measured pain point.** Teams with 6 services and no mTLS requirement rarely get positive ROI from mesh operational overhead.
- **Underestimating control-plane ownership cost.** A mesh needs a named owning team; "everyone owns it" in practice means nobody debugs it well when it breaks at 2am.
- **Big-bang rollout across every service at once.** Makes it impossible to isolate whether a new latency/error pattern came from the mesh or from something else, and risks a mass outage from one misconfiguration.
- **Putting business logic into mesh routing rules.** Mesh-level traffic rules should stay transport-level (retries, timeouts, routing percentages) — domain-aware retry/idempotency logic belongs in the application.
- **Ignoring sidecar resource overhead in capacity planning.** Every pod now needs extra CPU/memory for its sidecar; not budgeting for this causes surprise resource pressure at scale (see `capacity-planning-and-scaling-model`).
- **Treating mesh mTLS as sufficient security by itself** without also addressing application-level authorization — transport encryption between services isn't the same as verifying a caller is authorized for a specific action.
- **No rollback plan for a bad mesh config push.** A misconfigured traffic rule or mTLS policy pushed mesh-wide can take down inter-service communication across the whole cluster; require the same staged-rollout discipline for mesh config as for application deploys.

## Output

A service mesh evaluation document (e.g., `docs/architecture/service-mesh-evaluation.md`) recording: the specific east-west traffic pain points measured today, a capability-by-capability comparison of mesh vs. alternatives with the reasoning for each, the go/no-go decision, chosen mesh (if any) with the specific reason over alternatives, and a staged rollout plan with measured success metrics — feeding into an ADR and into the platform team's operational runbook.
