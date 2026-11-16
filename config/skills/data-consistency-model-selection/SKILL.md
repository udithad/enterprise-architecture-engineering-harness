---
name: data-consistency-model-selection
description: "Chooses the right consistency model (strong, eventual, causal, read-your-writes) per use case instead of defaulting to one model everywhere, weighing correctness needs against latency and availability. Use when designing distributed data access, caching, or replication where the CAP/PACELC trade-off actually matters to the business outcome."
---

# Data Consistency Model Selection

Pick a consistency model deliberately per use case — strong consistency where correctness demands it, eventual or causal consistency where latency/availability matter more — instead of either defaulting to strong consistency everywhere (paying an availability/latency tax the business doesn't need) or eventual consistency everywhere (creating correctness bugs where it actually matters).

## When to use this

- A system spans multiple regions or replicas and the team hasn't explicitly decided which reads/writes need strong consistency versus which can tolerate staleness.
- A bug report describes a user seeing "impossible" data (a balance that went negative, an item they just added missing from a list) traceable to an unstated consistency assumption that didn't hold.
- A new feature needs to choose between a strongly consistent datastore (higher latency, lower availability during partitions) and an eventually consistent one (lower latency, higher availability, but staleness) and the trade-off hasn't been made explicit.
- A caching layer (`caching-layer-architecture`) or a CQRS split (`cqrs-pattern-application`) has introduced staleness windows that need to be bounded and communicated, not left as an unstated side effect.
- Multi-region deployment (`multi-region-deployment-architecture`) is being designed and cross-region consistency requirements need to be defined per data type before picking a replication strategy.

## Workflow

1. **Classify each data type/operation by its actual correctness requirement**, not by habit. Money movement, inventory decrement, and anything with a uniqueness constraint (usernames, idempotency keys) usually need strong consistency. Activity feeds, view counts, recommendation data, and most read-heavy denormalized views tolerate eventual consistency well.
2. **Name the specific consistency model needed, not just "strong" or "eventual" as a binary** — strict/linearizable consistency, causal consistency (writes that causally depend on each other are seen in order), read-your-writes (a user always sees their own prior write), monotonic reads (a user never sees data go backward in time), or bounded staleness (stale by at most N seconds). Different guarantees cost different amounts of latency/availability.
3. **Map the CAP/PACELC trade-off explicitly for each choice**: under a network partition, does this data type favor consistency (reject/block writes) or availability (accept writes, reconcile later)? Even without a partition, does it favor lower latency (P/EL side) or stronger consistency (C/EC side)? State this per data type rather than assuming one answer for the whole system.
4. **Choose the mechanism that delivers the chosen model**: synchronous quorum writes or a single-leader datastore for strong consistency; async replication with conflict resolution (last-write-wins, CRDTs, or application-level merge) for eventual consistency; session tokens or sticky routing for read-your-writes on an otherwise eventually consistent store.
5. **Bound staleness explicitly wherever eventual consistency is chosen** — "replicated within 2 seconds at p99" is a testable, communicable guarantee; "eventually" with no bound is not something callers can design around safely.
6. **Design conflict resolution deliberately for any multi-writer eventually consistent data** — last-write-wins is simple but silently drops concurrent updates; CRDTs preserve more information but constrain the data model; application-level merge (e.g., shopping cart union) requires domain-specific logic. Pick consciously, not by default.
7. **Communicate the chosen guarantee to every consumer of that data**, in API docs or a data contract, so downstream teams build against the real guarantee instead of assuming strong consistency by default (the most common and most dangerous silent assumption).
8. **Test the consistency model under real partition/failure conditions**, not just the happy path — a "strongly consistent" system that hasn't been tested under a network partition may have an unverified failure mode that violates its own guarantee exactly when it matters most.
9. **Revisit the choice if the read/write pattern shifts** — a feature that started as low-stakes (an eventually-consistent "like count") can become correctness-critical later (feeding into a payout calculation), and the consistency model needs to move with it.
10. **Document the decision as an ADR per data domain**, not buried in a single database's configuration — the choice affects API design, client expectations, and multi-region topology, so it needs visibility beyond the datastore's own config file.

## Example

Scenario: `WalletHub`, a fintech app, has two data types that were both, until recently, handled by the same eventually-consistent, multi-region replicated datastore: account balances and a "recent activity" feed.

Analysis:
- **Account balance**: requires strong consistency — a user must never see a stale balance that allows a double-spend, and a debit/credit pair must be atomic and immediately visible to any subsequent read from any region. Decision: move balance to a single-leader, strongly consistent datastore with synchronous writes; accept higher write latency (leader is in `us-east`) and reduced availability during a partition (writes from `eu-west` route to the leader or fail, rather than accepting locally and reconciling later — silent balance drift is unacceptable).
- **Recent activity feed**: tolerates eventual consistency well — showing a transaction 2 seconds late is a minor UX issue, not a correctness bug. Decision: keep on the existing multi-region eventually-consistent store, with a documented bound of "visible within 3 seconds at p99," and read-your-writes guaranteed via sticky session routing so a user who just made a transaction always sees it in their own feed immediately, even if other users see it with the normal staleness bound.
- **Conflict resolution**: balance writes go through a single leader, so multi-writer conflicts don't arise by construction. Activity-feed writes use append-only, order-independent inserts, sidestepping conflict resolution entirely (no field is being overwritten by concurrent writers).

Result: the balance API's docs now explicitly state "strongly consistent, single-region write path"; the activity-feed API docs state "eventually consistent, ≤3s replication lag at p99, read-your-writes guaranteed." A downstream reporting service that had been silently assuming strong consistency on the activity feed (and computing user-facing balances by summing feed entries — the actual bug that triggered this review) is redirected to read from the balance store directly instead.

## Common pitfalls

- **Assuming strong consistency by default without stating it**, so a downstream consumer builds correctness-critical logic on data that was actually only eventually consistent.
- **Applying one consistency model uniformly across a whole system**, either paying an unnecessary availability/latency tax everywhere, or accepting staleness where it's actually dangerous.
- **"Eventual consistency" with no stated staleness bound**, making it impossible for consumers to reason about worst-case behavior.
- **Last-write-wins conflict resolution chosen without noticing it silently drops concurrent updates** in a case where that data loss actually matters.
- **Never testing the consistency guarantee under a real network partition**, leaving an unverified gap between the claimed and actual guarantee.
- **Not revisiting a consistency choice as a data type's stakes grow** — what started as a harmless "like count" becomes a financial input without anyone re-evaluating its consistency needs.

## Output

A data consistency decision doc per data domain (e.g., `docs/architecture/consistency-<domain>.md` or folded into the relevant ADR) stating the chosen model, the specific guarantee (with a staleness bound where applicable), the CAP/PACELC trade-off made, the conflict-resolution mechanism, and the communicated contract for downstream consumers.
