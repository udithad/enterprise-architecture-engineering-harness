---
name: database-per-service-design
description: "Designs data ownership so each service owns its own schema/database exclusively, with all cross-service data access going through APIs or events instead of shared tables. Use when decomposing a monolith's shared database, when two services are coupled through direct table access, or when one service's schema migration is blocked by another team's queries against its tables."
---

# Database-Per-Service Design

Give each service exclusive ownership of its own data store — no other service touches its tables directly, ever, regardless of language or convenience. Every cross-service data need goes through that service's API or through events it publishes. This is what actually makes services independently deployable; without it, "microservices" sharing one database are a distributed monolith with extra network hops.

## When to use this

- A monolith's single database is being split alongside a service decomposition (see `microservices-decomposition`), and the team needs a concrete plan for which tables go where and how cross-boundary queries get replaced.
- A service can't change its schema (add a column, rename a table, add an index) without coordinating with another team, because that team's service queries the same tables directly.
- Two "independent" services are actually coupled through a shared database, and a deploy of one has broken the other via an unannounced schema change.
- A new service is being built and someone proposes it read directly from an existing service's tables "just for this one report" — this is exactly the shortcut that recreates the coupling problem.
- Reporting/analytics needs are currently met by querying production service databases directly, creating load and coupling that should instead go through a dedicated data pipeline (see `data-lake-vs-warehouse-architecture-choice`).
- A service's data has a fundamentally different consistency, scaling, or compliance profile than its neighbors (e.g., one needs strong ACID guarantees, another needs to scale horizontally) and forcing them into the same database prevents choosing the right store for each.

## Workflow

1. **Assign every table to exactly one owning service.** For each entity, identify the single service responsible for validating and writing it — this determines placement, not which service currently happens to read it most. Ambiguous ownership ("both Orders and Billing write to `invoices`") is itself a signal the bounded contexts aren't cleanly split yet (see `domain-driven-design-context-mapping`).
2. **Enumerate every current cross-table access that crosses a proposed service boundary** — direct joins, foreign keys, or queries spanning what will become two databases. Each one needs a replacement: a synchronous API call, a published event a consumer projects into its own local copy, or (rarely, deliberately) acceptance that the query moves to a batch/analytics pipeline instead of live production traffic.
3. **Replace cross-service joins with either synchronous calls or local read replicas of foreign data.** If Service A frequently needs data owned by Service B for its own reads, don't have A query B's database — have A consume B's published events and maintain its own local, eventually-consistent projection of just the fields it needs (a pattern related to `cqrs-pattern-application`), or call B's API synchronously if strong consistency and low call volume make that acceptable.
4. **Decide per-service what a "foreign key" becomes.** Across a database boundary, a foreign key is no longer enforceable by the database — it becomes an opaque ID plus an application-level contract (the referenced service is the source of truth, and referential integrity is enforced by that service's API, or accepted as eventually consistent via events).
5. **Choose each service's data store based on its own needs, not organizational uniformity.** One service may need a relational store for transactional invariants, another a document store for flexible schema, another a wide-column store for time-series data — the whole point of per-service ownership is that this choice is now decoupled per service (see `data-consistency-model-selection`).
6. **Handle multi-service transactions explicitly** — once each service has its own database, a single ACID transaction across two services is no longer possible. Design the cross-service consistency strategy deliberately: a saga with compensating actions (see `saga-pattern-for-distributed-transactions`) for anything that needs multi-step coordination, or accept eventual consistency where the business tolerates it.
7. **Migrate incrementally using the strangler/branch-by-abstraction approach**, not a big-bang cutover: introduce a service-owned API in front of the still-shared tables first, migrate all consumers to that API, then physically split the database once nothing queries it directly anymore (see `monolith-to-microservices-migration-plan`).
8. **Cut off direct database access physically, not just by convention.** Use separate database instances/credentials per service (not just separate schemas in a shared instance that other teams could still be granted access to) so the boundary is enforced by infrastructure, not by a wiki page nobody reads.
9. **Design reporting/analytics access as a separate concern from the start.** Cross-service reporting queries are a legitimate need but should go through change-data-capture into a data warehouse/lake, not through direct production database queries against every service (see `data-lake-vs-warehouse-architecture-choice`).
10. **Plan for the operational cost increase explicitly.** N databases instead of 1 means N backup/restore procedures, N sets of credentials to rotate, N schemas to migrate — budget for this in the platform team's operational load, don't assume it's free.

## Example

Scenario: `ShopMono`'s single Postgres database has `orders`, `order_items`, `inventory`, `products`, and `shipments` tables. The `Checkout Service` (being extracted per the `microservices-decomposition` example) currently joins directly across `orders`, `inventory`, and `products` in one query to render the checkout summary page, and the `Shipping` team's reporting job queries `orders` directly every night.

Redesign:

| Table | New owner | Cross-boundary access replaced by |
|---|---|---|
| `orders`, `order_items` | Checkout Service | — (owns them outright) |
| `inventory` | Inventory Service | Checkout calls `Inventory.checkAvailability(sku)` synchronously during checkout (needs a real-time answer before confirming order) |
| `products` | Catalog Service | Checkout maintains a local, eventually-consistent `product_summary` projection (name, price, image URL) built by consuming `ProductUpdated` events — avoids a synchronous call on every checkout page render for data that changes rarely |
| `shipments` | Shipping Service | Shipping's nightly reporting job now reads from a data warehouse fed by CDC off Checkout's `orders` table, instead of querying Checkout's production database directly |

Rejected alternative: giving Checkout Service a read replica of the full shared Postgres instance. Rejected because a read replica still couples Checkout's query patterns to Inventory's and Catalog's internal schema — a column rename in either would silently break Checkout, exactly the coupling the redesign is meant to eliminate.

Foreign key handling: `order_items.sku` was previously a foreign key into `products.sku` enforced by Postgres. After the split, it becomes a plain string column; referential validity is enforced by Catalog Service's API returning a 404 for an unknown SKU at order-creation time, and Checkout's local `product_summary` projection is kept in sync via events rather than relying on a database constraint.

## Common pitfalls

- **"Just this one query" direct access exceptions.** Every direct cross-service database query granted "just this once" becomes permanent and recreates the coupling the pattern is meant to prevent.
- **Shared database instance with separate schemas as a stand-in for real separation.** If another team's credentials *can* reach your tables, the boundary is social convention, not architecture — enforce it with separate instances/credentials.
- **Treating foreign keys as still enforceable across the boundary.** Once split, referential integrity across services is an application-level contract (API validation or eventual consistency), not a database guarantee — designs that assume otherwise will silently drift.
- **No plan for multi-service transactions before the split happens.** Discovering mid-migration that `PlaceOrder` needs atomic updates across three now-separate databases, with no saga design ready, stalls the migration.
- **Production databases doubling as the analytics/reporting source.** Direct reporting queries against a live service's database create load coupling and schema coupling that CDC-into-a-warehouse avoids.
- **Underestimating operational overhead.** N services means N backup schedules, N sets of migration tooling, N monitoring dashboards — teams that don't plan for this get paged by databases nobody remembered to configure properly.
- **Splitting the database before the domain boundary is validated.** Physically splitting data storage locks in a boundary; if the bounded context mapping (`domain-driven-design-context-mapping`) is still shaky, the physical split becomes expensive to undo.

## Output

A data-ownership map (e.g., `docs/architecture/data-ownership.md`) listing every table/entity with its single owning service, every cross-boundary access pattern with its replacement mechanism (API call, event projection, or CDC-to-warehouse), the foreign-key-to-application-contract mapping, and the migration sequencing (strangler steps) — feeding into ADRs and directly into each service's data-access layer implementation.
