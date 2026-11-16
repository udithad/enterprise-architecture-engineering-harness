---
name: multi-tenant-architecture-design
description: "Chooses and enforces a tenant-isolation model (silo, pool, or hybrid) for a multi-tenant SaaS system based on tenant count, size variance, and compliance needs. Use when designing a new multi-tenant system, when a noisy tenant is degrading others, or when a tenant needs to move between isolation tiers."
---

# Multi-Tenant Architecture Design

Decide how much of the stack — compute, database, schema — is shared across tenants versus dedicated per tenant, and make tenant isolation an enforced architectural property rather than a convention developers are trusted to follow in every query.

## When to use this

- A SaaS system is being designed from scratch and the team needs to pick between shared-schema, schema-per-tenant, and database-per-tenant before the data model is set.
- One large tenant's traffic or query patterns are degrading performance for every other tenant on shared infrastructure (a noisy-neighbor problem).
- A prospective enterprise customer requires data residency, a dedicated database, or contractual isolation guarantees that the current shared-pool model can't offer.
- A cross-tenant data leak (or a near-miss) reveals that tenant isolation was enforced only by convention (developers remembering to add `WHERE tenant_id = ?`) rather than by the system.
- A growing tenant needs to migrate from a shared/pooled tier to a dedicated/siloed tier without downtime.
- Product wants per-tenant customization (custom fields, workflows, branding) and the data model needs to support that without one tenant's schema change affecting another.

## Workflow

1. **Place the system on the isolation spectrum first, as a range, not a single point.** Silo (dedicated database, and often dedicated compute, per tenant) gives the strongest isolation and blast-radius containment but multiplies operational cost — a schema migration must fan out across every tenant's database. Pool (one shared schema, every table carries a `tenant_id` column) is cheapest to operate and scales to many small tenants but requires airtight enforcement to prevent cross-tenant leakage and is exposed to noisy-neighbor effects. Bridge/hybrid (shared compute, per-tenant schema or per-tenant-tier database) sits between the two. Most real systems land on a hybrid: pooled by default, siloed for specific tenants or tiers.
2. **Base the choice on tenant count and size variance, not aesthetics.** Thousands of small, similar-sized tenants favor pooling (siloing thousands of databases is an operational tax nobody wants). A handful of large, high-value tenants with divergent scale and compliance needs favor siloing per tenant. Most B2B SaaS products have both shapes simultaneously and need a tiered model, not one answer for the whole product.
3. **Treat compliance and data residency as a hard constraint, not a preference.** If a tenant's regulatory regime requires their data to live in a specific region or be physically separated from other tenants' data, that tenant is siloed by requirement regardless of what the cost model would otherwise suggest.
4. **Enforce isolation at the system level, not the query-author level.** In a pooled model, every query must be scoped by tenant. Don't rely on every developer remembering the `WHERE tenant_id = ?` clause: enforce it with database row-level security (Postgres RLS policies keyed on a session variable), a query-building layer that injects the tenant predicate unconditionally, or a middleware-managed tenant context that a data-access layer cannot bypass. A single missed predicate in a pooled model is a cross-tenant data leak, not a bug ticket.
5. **Propagate tenant context explicitly through the whole request path.** Resolve tenant identity once (subdomain, JWT claim, API key) at the edge, and pass it as an explicit, unforgeable context object through every layer and into every downstream call and async job — never let a service infer tenant identity implicitly from data it happens to load.
6. **Design for the noisy-neighbor problem explicitly.** In pooled compute, one tenant's expensive query or traffic spike can degrade every co-located tenant. Mitigate with per-tenant quotas and throttling (see `rate-limiting-strategy-design`), per-tenant connection pool caps, query timeouts, and — for tenants that repeatedly cause trouble — promotion to a dedicated resource pool or silo.
7. **Plan tenant lifecycle operations up front**, not as an afterthought: automated provisioning (schema/database creation, seed data) on signup, and a clean, auditable offboarding path (full data export, then verifiable deletion) for compliance-driven termination requests.
8. **Design metering per tenant from day one**, even if billing isn't per-usage yet. Retrofitting per-tenant resource attribution (compute, storage, API calls) onto a system that never tracked it is far harder than instrumenting it from the start — see `cost-aware-architecture-design`.
9. **Treat tier migration as a designed operation, not a one-off script.** A tenant outgrowing the pooled tier needs a documented, ideally automated, path to a dedicated schema or database — including how existing data is copied/cut over with minimal downtime — because this will happen repeatedly as the product succeeds, not once.
10. **Decide the consistency model per shared resource explicitly.** Some data may need to be genuinely global (e.g., a shared product catalog referenced by all tenants) even in an otherwise siloed system — resolve up front whether that's replicated per tenant or served from one shared source, since getting this wrong either duplicates data unnecessarily or reintroduces a coupling point across tenants (see `data-consistency-model-selection`).
11. **Validate against the same test microservices-decomposition uses for service boundaries, applied to tenants**: can one tenant be moved, scaled, or deleted independently without needing coordinated changes affecting another tenant's behavior or data? If not, the isolation boundary is leaking.

## Example

Scenario: `Vantage`, a B2B analytics SaaS, has 4,000 small-to-mid customers (Free/Pro tiers) and 15 large Enterprise customers with contractual data-residency and dedicated-capacity requirements.

Isolation model chosen, per tier:

| Tier | Isolation model | Data storage | Rationale |
|---|---|---|---|
| Free (3,200 tenants) | Pool | Shared Postgres cluster, shared schema, `tenant_id` on every table, RLS policy enforced | Volume too high to silo economically; tenants are small and homogeneous |
| Pro (800 tenants) | Pool, with per-tenant resource quotas | Same shared cluster, RLS, plus per-tenant query timeout and connection cap | Same cost profile as Free but needs stronger noisy-neighbor protection since Pro tenants run heavier analytics queries |
| Enterprise (15 tenants) | Silo | Dedicated Postgres instance per tenant, dedicated app-tier resource pool | Contractual data residency and isolation requirements; low tenant count makes per-tenant infra cost acceptable |

Request flow for a pooled tenant:

```
Request --> Edge (resolve tenant from subdomain: acme.vantage.io -> tenant_id=T-4821)
        --> Auth middleware (verify JWT, bind tenant_id into unforgeable request context)
        --> App layer (all queries built through DAL that injects tenant_id predicate)
        --> Postgres (RLS policy additionally enforces tenant_id at the row level, defense in depth)
```

Rejected alternatives:

| Alternative | Rejected because |
|---|---|
| Silo every tenant from day one | At 4,000 small tenants, provisioning and migrating 4,000 databases for every schema change was operationally infeasible and made the cost-per-tenant unworkable for the Free tier |
| Pool every tenant, including Enterprise | Enterprise contracts explicitly required data residency guarantees and isolation from a shared cluster; a shared model couldn't satisfy the signed terms regardless of engineering preference |
| Rely on application-level `WHERE tenant_id` discipline only, no RLS | A near-miss incident where a new engineer's query omitted the tenant predicate and returned cross-tenant rows in a debug tool made "developer discipline" an unacceptable sole control; RLS was added as an enforced second layer |

When a Pro tenant's usage grows past the pooled tier's resource ceiling, an automated migration playbook provisions a dedicated schema, backfills their data via logical replication, and cuts traffic over with a brief maintenance window — this has run for 6 tenants promoted to Enterprise in the last year.

## Common pitfalls

- **Enforcing tenant isolation only in application code.** A missed `WHERE tenant_id` clause in a pooled model is a cross-tenant data leak; enforce isolation at the database level (RLS) as well as the application level.
- **One isolation model for a tenant base with wildly different sizes.** Forcing thousands of small tenants into per-tenant silos, or forcing a handful of compliance-bound whales into a shared pool, both fail — match the model to the tenant, not the product as a whole.
- **No plan for noisy neighbors.** A shared pool without per-tenant quotas, timeouts, and connection caps means one tenant's bad query degrades everyone else's experience, and it will happen.
- **Treating tenant migration between tiers as a one-off script instead of a repeatable operation.** It will need to run again as more tenants grow past the pooled tier's limits.
- **No per-tenant usage attribution until billing needs it.** Retrofitting metering onto a system that never tracked per-tenant resource consumption is much more expensive than instrumenting it at build time.
- **Implicit tenant context inferred from loaded data.** If tenant identity isn't explicitly and unforgeable-y threaded through the request path, a bug in one layer can silently operate on the wrong tenant's data.
- **Ignoring compliance/data-residency requirements until an enterprise deal is at risk.** These requirements should shape the isolation model up front for any tenant segment likely to need them, not get discovered during contract negotiation.

## Output

A multi-tenancy design document (e.g., `docs/architecture/multi-tenancy-model.md`) specifying: the isolation model per tenant tier with rationale, the tenant-context propagation mechanism, the enforcement layer(s) preventing cross-tenant access (application + database), the noisy-neighbor mitigation controls, the tenant provisioning/offboarding automation, and the documented migration path between tiers — feeding into the data model, the request-routing/middleware layer, and per-tenant billing instrumentation.
