---
name: nfr-to-architecture-mapping
description: "Translates non-functional requirements (availability, latency, throughput, security, compliance) into concrete architectural decisions and components, instead of leaving NFRs as vague adjectives nobody designed against. Use when a spec lists NFRs but the architecture hasn't been explicitly checked against each one."
---

# NFR to Architecture Mapping

Turn each non-functional requirement into a specific, testable architectural decision — a redundancy pattern for availability, a caching or indexing strategy for latency, a partitioning scheme for throughput — so NFRs stop being a paragraph nobody designed against and become traceable line items in the architecture.

## When to use this

- A requirements or spec document lists NFRs ("99.9% available," "sub-200ms p99 latency," "GDPR compliant") but the architecture document doesn't say how each one is met.
- An architecture review board (`architecture-review-board-prep`) asks "how does this design meet the latency requirement?" and the honest answer is "we assumed it would."
- A system passed functional QA but failed a later load test or availability drill — a sign the NFRs were never mapped to specific design decisions in the first place.
- `capacity-and-scale-requirements` or `sla-definition-workshop` (requirements-side skills) have produced numeric targets that now need to become architecture.
- Preparing a design doc and wanting to show explicit traceability from each NFR to the component/pattern that satisfies it, for review or audit purposes.

## Workflow

1. **Extract every NFR as a specific, numeric, testable statement** — reject vague inputs like "fast" or "scalable" and push for "p99 latency ≤ 200ms at 5,000 RPS" or "handles 3x current peak load with headroom." An NFR that can't be measured can't be mapped to a design or verified later.
2. **Group NFRs by category**: availability/reliability, latency/performance, throughput/scale, security/compliance, maintainability/operability, cost. Different categories map to different architectural levers.
3. **For each NFR, name the specific architectural pattern or component that satisfies it**, not a vague gesture at "good engineering": e.g., availability → redundancy + `high-availability-architecture-design`; latency → `caching-layer-architecture` + index design + geographic placement; throughput → horizontal scaling + partitioning; compliance → `security-architecture-review` + data residency controls.
4. **Check for conflicts between NFRs before finalizing the mapping** — strong consistency (a correctness NFR) often trades against low latency and high availability (see `data-consistency-model-selection`); a cost ceiling can conflict with an aggressive availability target. Name the conflict and the chosen trade-off explicitly rather than silently picking one.
5. **Assign an owner and a verification method to each NFR** — a load test for throughput, a chaos/failover drill for availability, a latency budget breakdown for performance, a pen test or compliance audit for security. An NFR without a verification method is a hope, not a requirement.
6. **Build a traceability table**: NFR → architectural decision(s) → component(s) → verification method. This is the artifact that makes NFR satisfaction auditable instead of assumed.
7. **Flag NFRs that the current architecture cannot meet** as early as possible — it's far cheaper to discover "this design can't hit 99.99% availability without a multi-region rework" during design review than after launch.
8. **Re-run the mapping when NFRs change** — a downgraded availability target might allow removing costly redundancy; a new compliance requirement might force a new data residency boundary. Treat NFRs as living inputs to architecture, not a one-time gate.
9. **Feed unmet or newly discovered NFRs back to requirements** (`requirement-change-impact-analysis`) when the architecture reveals a target is infeasible at the stated cost/timeline, rather than silently under-delivering.

## Example

Scenario: `Ledger`, an internal expense-approval system, has these stated NFRs: 99.9% availability, p95 API latency under 300ms, support 500 concurrent users at peak (quarter-end), SOC 2 compliance, and a $2,000/month infra budget ceiling.

Mapping:

| NFR | Architectural Decision | Component | Verification |
|---|---|---|---|
| 99.9% availability | Multi-AZ deployment (not multi-region — budget ceiling rules it out), health-checked auto-restart, no single point of failure in the request path | Load-balanced app tier across 2 AZs, managed RDS with a standby replica | Quarterly failover drill; measured uptime dashboard |
| p95 < 300ms | Read-through cache for approval-chain lookups (called on every request, rarely changes); DB index on `(approver_id, status)` | Redis cache layer (`caching-layer-architecture`), composite DB index | Load test at target RPS with p95 asserted in CI performance gate |
| 500 concurrent users at peak | Horizontal auto-scaling on the app tier; connection pooling to avoid DB connection exhaustion | Auto-scaling group (2–6 instances), PgBouncer | Load test simulating quarter-end spike pattern |
| SOC 2 compliance | Audit logging of every approval action; encryption at rest and in transit; RBAC on admin functions | Structured audit log to a write-once store; TLS everywhere; role-based access control layer | Annual SOC 2 audit; access-control review checklist |
| $2,000/month ceiling | Multi-AZ (not multi-region) chosen specifically because a full second region would exceed budget; cache sized to right-size DB instance tier down | Cost dashboard tied to the above components | Monthly cost review against ceiling |

Conflict surfaced: 99.9% availability and the budget ceiling together ruled out multi-region DR — this is stated explicitly in the design doc as "multi-region considered and rejected due to budget; residual risk: a full regional outage exceeds RTO. Accepted by [sponsor] on [date]," rather than left implicit.

## Common pitfalls

- **Leaving NFRs as unmapped prose in a requirements doc** while the architecture doc never references them — the two documents drift apart silently.
- **Vague NFRs that can't be verified** ("should be fast," "should scale") produce vague architecture that can't be checked either.
- **Ignoring conflicts between NFRs** instead of naming the trade-off and getting explicit sign-off — the conflict surfaces later as an incident instead of a documented decision.
- **No verification method per NFR**, so "meets the NFR" is asserted rather than demonstrated, and the gap is discovered in production.
- **Treating the mapping as a one-time exercise** rather than re-running it when NFRs or scale assumptions change.
- **Designing for NFRs that were never actually validated with the business** — over-engineering (unneeded five-nines availability) is as much a mapping failure as under-engineering.

## Output

An NFR traceability table or section in the architecture doc: each NFR stated numerically, its mapped architectural decision(s) and owning component(s), any conflicts with other NFRs and the trade-off made, and the verification method used to confirm it's actually met.
