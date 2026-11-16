---
name: monolith-to-microservices-migration-plan
description: "Sequences an incremental, low-risk migration from a monolith to services, with concrete milestones, rollback points, and a strangler-based cutover order. Use when leadership has approved decomposing a monolith and the team needs an executable roadmap, not just a target architecture diagram."
---

# Monolith-to-Microservices Migration Plan

Produce a phased, milestone-driven roadmap for extracting services out of a monolith incrementally, so the system stays shippable and rollback-able at every step instead of requiring a risky big-bang rewrite.

## When to use this

- Service boundaries have already been decided (see `microservices-decomposition`) and now the team needs to sequence the actual extraction work.
- Leadership asks "how long will this take and what do we ship along the way?" and a target-architecture diagram alone doesn't answer it.
- A previous migration attempt stalled or was reverted because it tried to move too much at once.
- The team needs to keep shipping features in the monolith while migration work proceeds in parallel.
- A migration needs an explicit rollback plan before anyone will approve touching the production critical path.

## Workflow

1. **Rank candidate services by extraction risk and value**, not decomposition-cleanliness alone. Score each on: blast radius if it breaks, coupling to the rest of the monolith (fan-in/fan-out), and business value of decoupling it (e.g., a team blocked on deploys, a compliance boundary). Extract low-risk, high-value services first to build confidence and reusable tooling.
2. **Pick the first service as a "path-finder."** It should be real but not mission-critical — extracting it proves out the CI/CD pipeline, service-to-service auth, observability wiring, and data-migration tooling that every subsequent extraction will reuse.
3. **Apply the strangler fig pattern per service** (see `strangler-fig-migration-pattern` for the mechanics): introduce a routing façade in front of the monolith, implement the new service, redirect a slice of traffic, verify, then redirect the rest and remove the old code path.
4. **Plan the data migration explicitly for each service**: will the new service own a copy (synced via CDC or dual-write during transition) or the single source of truth from day one? Dual-writing is usually necessary temporarily but must have an explicit end date — permanent dual-writes are a common failure mode.
5. **Define a rollback point for every phase.** Each extraction milestone should specify: the traffic percentage that triggers automatic rollback (e.g., error rate > 1% for 5 minutes), and the exact mechanism to revert (flip the façade's routing flag back to the monolith).
6. **Sequence dependent extractions correctly.** If Service B calls into logic that's still inside the monolith and destined to become Service C, either extract C first or have B call the monolith via a stable internal API that doesn't change shape when C is later extracted.
7. **Set a concrete timeline with checkpoints**, not just an end date — e.g., "Month 1: pathfinder service in strangler mode at 10% traffic; Month 2: pathfinder at 100%, monolith code removed; Month 3: begin extraction of Service 2." Each checkpoint should be a demonstrable, shippable state.
8. **Track monolith shrinkage as a metric** (lines of code, number of routes/tables still owned) so the migration's progress is visible beyond "productivity feels better."
9. **Explicitly plan the "boring middle"**: most migrations stall not on the first extraction (novel, gets attention) but the fourth or fifth (routine, deprioritized against feature work) — protect ongoing migration capacity (e.g., a fixed % of each sprint) rather than treating it as a one-off project.
10. **Define "done"**: what conditions mean the monolith is fully decommissioned or intentionally kept as a "core" service, and don't let the migration run indefinitely without a defined end state.

## Example

Scenario: `retailmono`, a 12-year-old monolith, needs to become 5 services: Catalog, Cart, Checkout, Inventory, Shipping (from a prior decomposition exercise).

Migration roadmap:

```
Phase 0 (Weeks 1-3): Foundations
  - Stand up API gateway/façade in front of retailmono (see api-gateway-design)
  - Build shared CI/CD template, service mesh sidecar config, observability
    stack (tracing, dashboards) to be reused by every extraction
  - Success gate: façade passes 100% of existing traffic through unchanged,
    verified by shadow-traffic diffing for 1 week

Phase 1 (Weeks 4-8): Pathfinder — Shipping Service
  - Chosen because: lowest fan-in (only Checkout calls it), no compliance
    sensitivity, clear data ownership (shipments table already isolated)
  - Dual-write shipments table to new service's Postgres instance for 2 weeks
  - Façade routes 10% of shipment-status-lookup traffic to new service,
    diff results against monolith nightly
  - Ramp to 100% over 1 week if error rate stays < 0.1%
  - Rollback trigger: error rate > 1% for 5 min -> façade flag reverts to monolith
  - Remove shipment code + table from monolith after 2 weeks at 100% clean

Phase 2 (Weeks 9-16): Inventory Service
  - Higher risk: Checkout depends on synchronous stock reservation
  - Extract with dual-write + CDC-based sync (see cdc-change-data-capture-design)
  - Requires Checkout to switch from in-process call to a synchronous gRPC
    call with circuit breaker (see circuit-breaker-pattern-design) before cutover

Phase 3-5 (Weeks 17-30): Checkout, Cart, Catalog
  - Checkout last among the "core" flow services since it has the highest
    fan-in and the most business risk if broken

Ongoing: 20% of each sprint reserved for migration work throughout, to avoid
the "boring middle" stall after the pathfinder's launch excitement fades.
```

## Common pitfalls

- **Starting with the most valuable service instead of the safest one.** High-value targets are usually also high fan-in and high risk — build muscle memory on something safer first.
- **Big-bang cutover instead of traffic-percentage ramping.** Skipping shadow traffic and percentage rollout removes your safety net and turns every extraction into a one-shot bet.
- **Permanent dual-writes.** Dual-writing data between the monolith and new service without a firm decommission date becomes silent, permanent technical debt and a data-consistency risk.
- **No reserved ongoing capacity.** Migrations that compete sprint-by-sprint against feature work reliably stall after the first one or two extractions.
- **Ignoring inter-service dependency order.** Extracting a service whose logic another not-yet-extracted service depends on internally creates awkward temporary coupling that's easy to get wrong.
- **No explicit rollback trigger.** "We'll roll back if something looks bad" is not a plan; define the metric and threshold before cutover, not during an incident.
- **Treating the plan as fixed.** Revisit sequencing after each phase — what you learn from the pathfinder service should adjust the plan for the next one.

## Output

A migration roadmap document (e.g., `docs/architecture/migration-plan.md`) with: risk/value-ranked service extraction order and rationale, a phase-by-phase timeline with concrete milestones and success/rollback gates, the data-migration approach per service, and a named "done" condition — plus a lightweight dashboard or metric tracking monolith shrinkage over time.
