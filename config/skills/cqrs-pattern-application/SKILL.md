---
name: cqrs-pattern-application
description: "Splits a service's write model from its read model so each can be scaled, denormalized, and evolved independently, instead of one schema serving both. Use when read and write workloads have very different shapes, when a single normalized schema can't serve all query patterns efficiently, or when read scaling is being bottlenecked by write-model locking."
---

# CQRS Pattern Application

Separate the model that handles commands (validates business rules, mutates state) from the model that serves queries (denormalized, shaped for exact read patterns), connected by a projection mechanism instead of one shared schema trying to serve both. This is a targeted tool for a specific symptom, not a default architecture — apply it to the bounded contexts that actually show the symptom, not the whole system.

## When to use this

- A service needs to support 5+ very different read patterns (dashboard aggregates, search, per-user history, admin reports) off the same write data, and one normalized schema requires expensive joins or read-time computation for each.
- Write throughput is being throttled by read-query lock contention on the same tables, or vice versa — reads and writes are fighting over the same rows/indexes.
- The read side needs a different storage technology than the write side (e.g., writes go to Postgres for transactional integrity, reads need Elasticsearch for full-text search or a wide denormalized table for a dashboard).
- The team is already using event sourcing for the write side and needs a queryable view — CQRS's read projections are the natural way to expose event-sourced state as queryable data.
- A domain has commands with real business invariants (e.g., "can't overdraw an account") but reads that just need a fast, eventually-consistent view — the validation-heavy write path and the fast-serve read path have genuinely different design pressures.
- Someone proposes CQRS for a simple CRUD service with one read pattern and no scaling pain — that's a signal to push back, not adopt it (see Common pitfalls).

## Workflow

1. **Confirm the symptom before reaching for the pattern.** CQRS adds a second model, a projection mechanism, and eventual consistency between them — real, ongoing complexity. Verify the read/write shape mismatch or scaling bottleneck is real (measure query latency, lock wait times, schema complexity) before committing; don't adopt it because it's an interesting pattern.
2. **Scope CQRS to the bounded context that needs it**, not the whole system. Apply it per aggregate or per service (see `microservices-decomposition`) — a `Catalog` service with simple CRUD doesn't need it just because `Orders` does.
3. **Design the command side around business invariants.** Commands (`PlaceOrder`, `CancelSubscription`) are validated against the write model, which should be normalized and consistency-enforcing (typically a relational DB with the domain's actual invariants as constraints), not a copy of the read shape.
4. **Design each read model around one specific query pattern**, denormalized deliberately. A `Dashboard` read model, an `OrderHistory` read model, and a `SearchIndex` read model can be three different physical stores (Postgres materialized view, DynamoDB table, Elasticsearch index) each shaped for exactly one access pattern — don't try to make one read model serve every query.
5. **Choose the projection mechanism.** Options: synchronous dual-write (simple, but risks the read model and write model drifting out of sync on partial failure — generally avoid), transactional outbox + async projector (write model change is captured reliably and projected asynchronously — the standard approach), or event sourcing where the write model *is* the event log and read models are pure projections of it (see `event-driven-architecture-design` for the event mechanics).
6. **Make the staleness window explicit and visible where it matters.** Decide and document how long after a write the read model may lag (milliseconds to seconds is typical with a good outbox+projector setup) and make sure UX/API contracts don't silently assume immediate consistency (see `data-consistency-model-selection`). If a user must see their own write immediately, route that specific read back to the write model or add read-your-writes tracking.
7. **Handle projector failure and replay explicitly.** A read-model projector can and will fall behind or crash; design it to be idempotent and re-runnable from the last checkpoint, and plan for full read-model rebuilds from the event log/outbox when a projection's schema changes.
8. **Version read models independently of the write model.** A new denormalized view for a new UI feature is just a new projector reading the same event stream — it shouldn't require touching the write model or its invariants.
9. **Decide who owns the read-model schema.** Because read models are often optimized for a specific consumer (a frontend team's dashboard needs), consider letting that consuming team own and evolve their own projection rather than centralizing all read-model design in the service team.
10. **Instrument replication lag and projector health** as first-class metrics (see `observability-architecture-design`) — a silently stalled projector serving stale data for hours is a much worse failure mode than a visible error.

## Example

Scenario: `BookingCore`, a hotel-reservation service, has one Postgres schema serving both the booking write path (create/modify/cancel reservations, with real invariants like "no double-booking a room") and five read patterns: a customer-facing "my bookings" list, a hotel-ops dashboard showing today's occupancy, a search-by-date-and-city page, a finance reconciliation export, and an analytics pipeline. The occupancy dashboard query (a multi-table join across reservations, rooms, and rate plans) was taking 400ms+ and locking rows that the write path needed, causing booking-creation latency spikes during peak search traffic.

Applied CQRS:

| Model | Store | Shape | Consistency |
|---|---|---|---|
| Write model | Postgres (`reservations`, `rooms`, `rate_locks`) | Normalized, enforces "no double-booking" via a unique constraint on `(room_id, date_range)` | Strong (source of truth) |
| "My bookings" read model | Postgres materialized read table, projected via outbox | One row per booking, pre-joined with hotel name/address | Eventually consistent, ~1-2s lag |
| Occupancy dashboard read model | Denormalized DynamoDB table keyed by `hotel_id + date` | Precomputed occupancy counts | Eventually consistent, ~5s lag, acceptable for an ops dashboard |
| Search read model | Elasticsearch index | Full-text + geo + date-range search | Eventually consistent, ~2s lag |
| Finance export | Batch read from write model directly (no separate projection) | — | Rejected building a dedicated read model here — finance runs a nightly batch job and reads the write model directly; no scaling pain, no need for the extra projection |

Rejected alternative: synchronous dual-write from the API layer to both Postgres and DynamoDB. Rejected because a partial failure (Postgres commit succeeds, DynamoDB write fails) leaves the dashboard silently wrong with no recovery path; the transactional-outbox + async projector approach guarantees the read models eventually catch up because the outbox is written in the same transaction as the write-model change.

Result: booking-creation latency dropped from spiking under dashboard load to a flat baseline, since the dashboard query no longer touches the write-model tables at all.

## Common pitfalls

- **Applying CQRS everywhere "for consistency" of architecture.** Most CRUD endpoints have one read pattern and no scaling pain — adding a second model and a projector there is pure overhead with no corresponding benefit.
- **Synchronous dual-writes instead of a reliable outbox.** Writing to the write DB and read DB in the same request without a transactional guarantee produces silent drift the first time one write fails and the other succeeds.
- **No documented staleness contract.** Teams building against the read model assume strong consistency because nobody wrote down the lag, then ship a bug where a user doesn't see their own just-created booking.
- **One read model trying to serve too many query shapes.** Recreating a single "flexible" denormalized table for every consumer defeats the purpose — each read model should be shaped for one access pattern.
- **No replay/rebuild story.** When a read-model schema needs to change, teams without a replay plan resort to manual data migration scripts instead of just re-running the projector from the event log.
- **Conflating CQRS with event sourcing.** CQRS (separate read/write models) does not require event sourcing (write model as an append-only event log) — they're often paired but are independent decisions; forcing event sourcing on a team that only needed CQRS adds unnecessary complexity.
- **Ignoring projector lag under load.** A projector that falls behind during traffic spikes (exactly when the dashboard is most needed) without alerting turns a performance optimization into an incident.

## Output

A CQRS design document (e.g., `docs/architecture/cqrs-design.md`) per bounded context, specifying: the write model and its invariants, each read model with its store, shape, and consumer, the chosen projection mechanism (outbox/event-sourcing) with staleness bounds per read model, and the explicit read patterns considered and rejected as not warranting a dedicated model — feeding into ADRs and into the projector's operational runbook.
