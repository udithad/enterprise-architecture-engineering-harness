---
name: microservices-decomposition
description: "Splits a monolith or greenfield system into microservices along bounded-context and data-ownership lines rather than arbitrary technical layers. Use when a team is designing service boundaries for a new system or extracting services from an existing monolith and needs a defensible, not-guessed, decomposition."
---

# Microservices Decomposition

Decide where service boundaries go by finding where data ownership and business capability naturally split, not by cutting along technical layers (e.g., "auth service," "validation service") that force every feature to touch multiple services.

## When to use this

- A monolith has grown past the point where one team can safely deploy it, and leadership wants to split it into independently deployable services.
- A greenfield system is being designed and the team wants to avoid a "distributed monolith" from day one.
- Two teams keep blocking each other on deploys because they share a codebase with no clear ownership boundary.
- A specific subsystem (e.g., billing, notifications) has a very different scaling or compliance profile than the rest of the system and needs to be isolated.
- Someone proposes splitting services by technical layer ("frontend-service," "database-service") and you need a better decomposition axis to counter-propose.

## Workflow

1. **Model the domain with bounded contexts first**, not services. Run event storming or a lighter domain-mapping session to find where terminology and business rules change meaning across the organization (see `domain-driven-design-context-mapping`). A bounded context is a *candidate* service boundary, not automatically one.
2. **Identify data ownership clusters.** For each entity/table, ask which business capability is the single authoritative owner of writes to it. Entities that are always read and written together as a unit are a strong signal they belong in the same service.
3. **Trace the actual call graph and change history.** Look at which modules change together in commits (a "change coupling" analysis) — modules that are always modified in the same PRs are poor candidates for separate services regardless of what the org chart suggests.
4. **Draw candidate boundaries around business capabilities**, e.g., `Ordering`, `Inventory`, `Billing`, `Notifications` — not around technical concerns like `Validation` or `Auth-checks` (those become libraries or a shared gateway concern, not services).
5. **Check each candidate boundary against these tests:**
   - Can one team own it end-to-end (data, logic, deploy) without needing sign-off from another team for routine changes?
   - Does it have a materially different scaling, latency, or compliance profile than its neighbors?
   - Can you name its API contract in one paragraph without referencing another service's internals?
   - If the answer to any is "no" or "unclear," merge it back with a neighbor rather than splitting further.
6. **Design the API contract per service boundary** (see `api-versioning-strategy`, `grpc-vs-rest-vs-graphql-decision`) before writing implementation code.
7. **Decide data ownership rules explicitly**: which service owns which tables, and how other services get that data (synchronous API call vs. consuming published events vs. a replicated read model) — never direct cross-service database access (see `database-per-service-design`).
8. **Plan for the awkward cases up front**: cross-cutting concerns like audit logging, and multi-service transactions (see `saga-pattern-for-distributed-transactions`) — don't defer this until implementation reveals the problem.
9. **Start with fewer, coarser services** than you think you need. It's far easier to split a service later (once real seams appear) than to merge two services that were split prematurely and now have a chatty, tightly-coupled API between them.

## Example

Scenario: an e-commerce monolith (`shopmono`) handling catalog, cart, checkout, inventory, and shipping is being decomposed.

Domain mapping reveals these bounded contexts and their relationships:

```
Catalog (upstream)  --publishes ProductUpdated-->  Search Index (downstream, ACL)
Cart (upstream)     --calls-->                      Pricing (customer/supplier)
Checkout (upstream) --calls-->                      Inventory (customer/supplier)
Checkout            --publishes OrderPlaced-->       Shipping (downstream, event-driven)
Checkout            --publishes OrderPlaced-->       Billing (downstream, event-driven)
```

Proposed service boundaries and ownership:

| Service | Owns data | Rejected alternative split |
|---|---|---|
| Catalog Service | products, categories, pricing rules | splitting "pricing" out separately — rejected: pricing rules never change independent of product data, always co-modified |
| Cart Service | active carts, line items | — |
| Checkout Service | orders, order state machine | initially proposed merging with Inventory — rejected: checkout has web-facing latency SLOs, inventory has batch-reconciliation SLOs; different scaling needs |
| Inventory Service | stock levels, reservations | — |
| Shipping Service | shipments, carrier integration | — |
| Billing Service | invoices, payment records | — |

Checkout calls Inventory synchronously (`POST /reservations`) to reserve stock during the order flow (needs a strongly consistent answer before confirming the order), but publishes `OrderPlaced` as an event for Shipping and Billing to consume asynchronously (they don't block the customer-facing checkout latency).

## Common pitfalls

- **Decomposing by technical layer.** "Auth service," "logging service," "validation service" as top-level microservices force every business flow to hop between several services for one user action — these usually belong as libraries, sidecars, or gateway-level concerns instead.
- **Ignoring change-coupling data.** Org charts and diagrams can lie; git history rarely does. If two "separate" modules are modified together in 80% of PRs, they are one service pretending to be two.
- **Splitting before the domain model is stable.** Premature decomposition locks in service boundaries around a domain understanding that's still wrong; a few sprints of monolith-first development can reveal the real seams.
- **Shared database across "separate" services.** If two services both read/write the same table directly, they are not actually decoupled — see `database-per-service-design`.
- **No plan for cross-service transactions.** Decomposition that ignores how a `PlaceOrder` flow spanning Checkout, Inventory, and Billing stays consistent will hit this wall in week one of implementation.
- **Too many services too soon.** More services means more network calls, more deployment pipelines, and more operational overhead; a team of 6 engineers rarely needs more than 4-6 services to start.

## Output

A decomposition document (e.g., `docs/architecture/service-decomposition.md`) listing: each proposed service, its owned data/tables, its public API surface (one paragraph), its upstream/downstream relationships to other services (sync call vs. event), and the specific alternative splits considered and rejected with reasons — feeding directly into ADRs for each service boundary.
