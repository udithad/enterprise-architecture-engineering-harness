---
name: high-availability-architecture-design
description: "Designs a system to keep serving traffic correctly through routine failures (node crashes, AZ outages, dependency timeouts) by eliminating single points of failure and automating failover. Use when defining an uptime SLO and the redundancy architecture needed to hit it, or when reviewing a design for hidden SPOFs."
---

# High Availability Architecture Design

Keep the system serving correct responses through the failures that happen every week — a node dying, an AZ blipping, a dependency timing out — by removing single points of failure and making failover automatic and fast. This is about surviving routine failure, not recovering from a catastrophe; for that, see `disaster-recovery-architecture-planning`.

## When to use this

- A new SLO is being set (e.g., 99.95% availability) and someone needs to translate that number into concrete redundancy decisions.
- A postmortem revealed a single instance, single AZ, or single dependency that took the whole system down.
- A design review needs to check whether a proposed architecture has hidden SPOFs before it ships.
- Leadership asks "what happens if a node/AZ/dependency goes down right now" and nobody has a confident answer.
- A system is being justified for active-active vs active-passive redundancy and the cost/complexity tradeoff needs to be made explicit.
- Someone conflates "we have backups" (disaster recovery) with "we stay up when a server dies" (high availability) and the distinction needs to be drawn clearly.

## Workflow

1. **Set the availability target as a number, not a vibe.** Translate "highly available" into a concrete SLO (99.9%, 99.95%, 99.99%) and its downtime budget (99.95% = ~4.38 hours/year, ~21.6 minutes/month). Every redundancy decision downstream is justified against this number — don't over-engineer for five nines when the business tolerates three.
2. **Draw the request path end-to-end and mark every hop as a candidate SPOF.** Load balancer, app tier, cache, primary database, message broker, third-party dependency (payment gateway, auth provider) — anything with exactly one instance in the path is a SPOF until proven otherwise.
3. **Apply the availability-math rule: components in series multiply, redundant components in parallel add nines.** If a request depends on 4 services each at 99.9%, the composed availability is ~99.6%, not 99.9% — you cannot compose fragile parts and expect a robust whole. Redundant parallel paths (N+1, N+2) are how you claw the nines back.
4. **Size stateless tiers for N+1 or N+2, not N.** N+1 tolerates one failure while at full capacity; N+2 tolerates one failure during a rolling deploy plus one unplanned failure at the same time. Running at exactly N means any failure is already a capacity incident (see `capacity-planning-and-scaling-model` for how headroom is sized).
5. **Choose active-active vs active-passive per stateful component deliberately.** Active-active (all nodes serve traffic) gives better resource utilization and near-zero failover time but requires the component to tolerate concurrent writes or partitioning (quorum/consensus, e.g., Raft-based stores). Active-passive (standby absorbs traffic only on failover) is simpler and avoids split-brain but wastes standby capacity and has a nonzero failover window. Most primary relational databases end up active-passive with synchronous replicas for this reason.
6. **Design failure detection and automatic failover, not manual intervention.** Load balancer health checks (liveness + readiness, not just a TCP ping), a deregistration threshold tuned to avoid flapping (e.g., 3 consecutive failures before pulling a node), and automated leader election/promotion for stateful stores (e.g., Patroni for Postgres) so a human doesn't have to be paged to restore availability.
7. **Decide replication mode per data store based on what it protects against.** Synchronous replication (e.g., RDS Multi-AZ) gives zero data loss on failover but adds write latency and is usually confined to one region; asynchronous replication (cross-region read replicas) has lower latency but a replication-lag window of possible data loss on failover — that lag is a DR concern (RPO), not an HA one.
8. **Add circuit breakers and graceful degradation at every external dependency boundary** (see `circuit-breaker-pattern-design`). A slow or down dependency must fail fast and let the caller degrade (serve cached/stale data, skip a non-critical feature) rather than exhaust connection pools and cascade the outage upstream.
9. **Contain blast radius with bulkheads.** Separate connection pools, thread pools, or even separate deployments per tenant/customer tier so one noisy or failing consumer can't exhaust shared resources and take everyone down.
10. **Validate the design with chaos engineering, not just a diagram review.** Kill a node, block a dependency, saturate a connection pool in a controlled game-day exercise and confirm the system actually fails over within the assumed time — an HA design that has never been tested is a hypothesis.
11. **Decide whether multi-region is in scope, and for which purpose.** Multi-region active-active can serve HA (survive a regional outage with zero manual intervention) but costs roughly 2x infrastructure and forces a data-consistency strategy across regions (see `multi-region-deployment-architecture`, `data-consistency-model-selection`). Don't reach for it if a single-region, multi-AZ design already meets the SLO — that complexity is usually better spent elsewhere.
12. **Document the assumed failure modes and the SLO math** so the design can be audited later — feed this into an ADR (see `architecture-decision-record-authoring`) rather than leaving it as tribal knowledge.

## Example

Scenario: `payflow`, a payment-processing API, is given a 99.95% availability SLO (~21.6 min/month downtime budget) and currently runs as a single instance in one AZ with a single-AZ RDS Postgres primary.

Request path and redundancy decisions:

```
Client -> ALB (3 AZs) -> ECS app tier (N+2, 3 AZs) -> RDS Postgres Multi-AZ (sync standby) -> Stripe (external, circuit-breaker wrapped)
```

| Component | Redundancy chosen | Why | Rejected alternative |
|---|---|---|---|
| Load balancer | ALB across 3 AZs with health checks, 3-strike deregistration | Removes the LB and the AZ as SPOFs; standard managed HA | Single NLB in one AZ — rejected, reintroduces an AZ-level SPOF |
| App tier | ECS service sized for N+2 (6 tasks across 3 AZs, tolerates 2 simultaneous losses) | SLO budget doesn't tolerate capacity-driven latency spikes during a deploy + node loss | N (4 tasks) — rejected, one AZ loss during a rolling deploy drops capacity below demand |
| Primary database | RDS Multi-AZ with synchronous standby, automated failover (~60-120s) | Payment writes cannot tolerate data loss on failover; sync replication is required | Async cross-region replica as the "HA" story — rejected, that's a DR pattern (RPO > 0), not HA; conflating the two was the original design mistake |
| External dependency (Stripe) | Circuit breaker with 2s timeout, fallback to "payment pending" queued retry | Stripe outages must not exhaust `payflow`'s connection pool or fail closed on customers | Synchronous retry-until-success — rejected, ties up threads and cascades the outage into `payflow` itself |
| Multi-region | Not adopted | Single-region multi-AZ design already models to 99.96% against the composed failure rates; regional outage is rare enough that DR (with its own RTO/RPO) is the right tool, not always-on multi-region | Active-active multi-region — rejected for now: ~2x cost and forces a cross-region consistency model for money-movement data that the business didn't ask for at this SLO |

Composed availability check: ALB (99.99%) x app tier N+2 (99.99%) x RDS Multi-AZ (99.95%) x Stripe circuit-breaker-degraded (treated as non-blocking) models to roughly 99.93-99.96%, comfortably inside the 99.95% target with margin for the app-level failure rate.

## Common pitfalls

- **Calling a single-AZ multi-instance deployment "highly available."** If every instance is in one AZ, an AZ-level event (power, networking) is still a SPOF regardless of instance count.
- **Confusing HA with DR.** An async cross-region replica is a DR asset (nonzero RPO, manual or semi-manual promotion); presenting it as the HA answer for a component that needs zero-data-loss failover is the single most common mistake in this space.
- **Sizing for N instead of N+1/N+2.** Running at exactly the capacity needed means any single failure is instantly also a capacity incident, not just an availability one.
- **No automated failover for stateful components.** If promoting a new database primary requires a human to run a runbook at 3am, the real MTTR is however long it takes to page and respond, not the failover mechanism's theoretical speed.
- **Ignoring composed availability math.** Five "99.9%" services chained in series do not compose to 99.9% — teams routinely overestimate system availability by reasoning about each component in isolation.
- **No blast-radius containment.** Shared connection pools and thread pools mean one slow dependency or noisy tenant degrades everyone, turning a local problem into a global outage.
- **Never testing the failover.** An HA design validated only on a whiteboard, never via chaos engineering or an actual game day, is an unverified assumption, not a working system.

## Output

An HA design document (e.g., `docs/architecture/high-availability-design.md`) stating the target SLO and its downtime budget, an annotated request-path diagram with every redundancy decision and its rejected alternatives, the composed-availability calculation showing the design meets the target, and the chaos/game-day plan used to validate it — typically feeding an ADR and the on-call runbook for automated failover behavior.
