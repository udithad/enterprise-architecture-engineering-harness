---
name: zero-downtime-deployment-architecture
description: "Architects application, database, and traffic-routing changes so releases never require downtime — covering backward-compatible schema migrations, rolling/blue-green deploys, and safe in-flight request handling. Use when a system currently requires a maintenance window to deploy, or when designing a new system that must never have one."
---

# Zero-Downtime Deployment Architecture

Architect a system so every release — application code, database schema, and configuration — can ship without a maintenance window, by making deployments rolling or blue-green, database migrations backward-compatible in stages, and in-flight requests handled safely across the version boundary, instead of relying on scheduled downtime to paper over unsafe changes.

## When to use this

- A system currently requires a maintenance window for releases, and business requirements (global users, SLA commitments, competitive pressure) no longer tolerate scheduled downtime.
- Designing a new system that must be zero-downtime from day one rather than retrofitting it later, which is significantly more expensive.
- A recent deployment caused an unplanned outage (not even during a maintenance window) due to an incompatible schema change or a version mismatch between old and new instances during a rolling deploy.
- Adopting `blue-green-deployment-strategy` or `canary-deployment-strategy` at the infrastructure level, and needing the application and database layers to actually support running two versions simultaneously.

## Workflow

1. **Accept the core constraint: two application versions run simultaneously during every rolling deploy**, even if briefly — old and new instances both serve traffic during the rollout window. Every change must be safe under that condition, not just safe as a before/after comparison.
2. **Make database schema changes backward- and forward-compatible in stages**, never a single atomic change that only one version understands. The standard pattern: (1) add a new column/table as nullable/optional — both old and new code tolerate its presence; (2) deploy new code that writes to both old and new fields (dual-write) or reads with a fallback; (3) backfill existing data; (4) deploy code that only uses the new field; (5) only once no code references the old field, remove it in a later, separate deploy.
3. **Never deploy a schema change and the code that depends on it in the same release** — sequence them across at least two separate deploys so there's always a window where the schema supports both the old and new code's expectations.
4. **Design API/contract changes to be backward-compatible during the transition** (reuse `backward-compatibility-strategy` and `api-versioning-strategy`) — a new required field, a renamed field, or a changed response shape must not break in-flight requests being served by the previous version, or clients that haven't yet picked up a new client version.
5. **Handle in-flight requests during instance replacement gracefully** — use connection draining (stop routing new requests to an instance being terminated, let existing requests finish within a bounded grace period) rather than abruptly killing instances mid-request.
6. **Design long-running or stateful operations to survive a deploy** — a request that spans multiple app-server hops, a long-running background job, or a websocket connection needs an explicit strategy (resumable jobs, sticky sessions with a defined fallback, reconnect logic) since a rolling deploy will eventually replace the specific instance handling it.
7. **Version any message/event schema consumed by async workers the same way** — a producer emitting a new event schema while a not-yet-updated consumer is still running (or vice versa) is the same compatibility problem as the synchronous API case, and needs the same staged approach.
8. **Automate a health-check-gated rollout** — new instances only receive traffic once they pass a real readiness check (not just "process started"), and the rollout pauses or auto-rolls-back if error rates spike, rather than blindly replacing all instances on a fixed timer.
9. **Test the two-versions-running-simultaneously state explicitly**, not just the before and after states — a staging environment deploy that briefly runs old and new versions side by side, with real traffic or synthetic checks, catches compatibility bugs that a "deploy and check it works" test misses.
10. **Keep a fast, well-tested rollback path** for every release — zero-downtime deployment isn't just about the forward path; a broken release still needs to be reversible without downtime, which requires the same backward-compatibility discipline in reverse.

## Example

Scenario: `OrderFlow`, an order-management system, needs to rename the `status` field on the `orders` table from a free-text string to a normalized `status_id` referencing a new `order_statuses` lookup table, without downtime.

Staged rollout across four separate deploys:

1. **Deploy 1 — additive schema change**: add the new `order_statuses` lookup table and a nullable `status_id` column on `orders`. No application code changes yet; both old and new schema states are valid for the currently-running (old) code, which only ever touches `status`.
2. **Deploy 2 — dual-write code**: deploy new application code that, on every order status update, writes both the legacy `status` string and the new `status_id`, and reads from `status` (the still-authoritative field) for all business logic. During this rollout's brief window where old and new instances run simultaneously, old instances still write only `status` and new instances write both — both are individually valid, and no reader depends on `status_id` yet, so there's no correctness gap.
3. **Backfill**: a background job populates `status_id` for all historical rows that predate deploy 2, run only after deploy 2 is fully rolled out (confirmed via a metric showing 100% of instances on the new version).
4. **Deploy 3 — cut over reads**: deploy code that reads `status_id` as authoritative and treats `status` as a derived/legacy field, still dual-writing both for safety. Rolled out the same way, tested for the simultaneous-old/new-version window.
5. **Deploy 4 — remove legacy field**: only after deploy 3 has been stable for a full release cycle and no code references `status` anymore, drop the dual-write and, in a later deploy, drop the `status` column itself.

Each deploy individually is a small, low-risk, backward-compatible change; the risky "rename a field" operation that would traditionally need a maintenance window is decomposed into four safe, independently-deployable, independently-rollback-able steps — reused as the standard pattern (`zero-downtime-schema-migration`-style staged migration) for every subsequent schema change on the team.

## Common pitfalls

- **Deploying a schema change and its dependent code together**, creating a window where either the old code breaks against the new schema or the new code breaks against the old schema during the rolling deploy.
- **Treating "works before and after" as sufficient testing**, missing bugs that only occur while old and new versions run simultaneously.
- **Abruptly terminating instances without connection draining**, dropping in-flight requests during every deploy even though no maintenance window was scheduled.
- **No plan for long-running requests, background jobs, or persistent connections** across a deploy, causing silent failures or dropped work that's hard to attribute to "just a normal deploy."
- **Treating rollback as an afterthought**, discovering during an actual incident that rolling back also requires downtime because the same forward-compatibility discipline wasn't applied in reverse.
- **Async event/message schema changes handled less carefully than synchronous API changes**, even though producer/consumer version mismatch is the same compatibility problem.
- **No automated health-check gating**, replacing instances on a fixed timer regardless of whether the new version is actually healthy, turning a routine deploy into an outage.

## Output

A zero-downtime deployment design covering: the staged schema-migration pattern for any given change, the API/event backward-compatibility approach during rollout, the connection-draining and health-check-gated rollout mechanism, handling for long-running/stateful operations across a deploy, and a tested, equally-safe rollback path — validated by explicitly testing the simultaneous old-and-new-version state, not just before/after.
