---
name: caching-layer-architecture
description: "Designs multi-tier caching (client, CDN, application, database) with an explicit invalidation strategy per tier, so caches speed the system up without serving stale or inconsistent data. Use when latency or database load is a problem, or when an existing cache is causing subtle staleness bugs."
---

# Caching Layer Architecture

Design caching as a deliberate multi-tier system with an explicit invalidation strategy per tier — not a `@Cacheable` annotation sprinkled wherever a query feels slow — so caching reduces latency and load without introducing the class of bug where users see stale or contradictory data.

## When to use this

- Database load or API latency is dominated by repeated reads of the same, rarely-changing data, and there's no caching layer (or an ad hoc one with no invalidation story).
- Users occasionally report seeing stale data (an old price, a stale profile) and nobody's sure which of several caches is holding it or for how long.
- A new feature needs to serve read-heavy traffic at a latency budget the primary datastore can't meet directly, even with query optimization (see `database-query-optimization`).
- Multiple services are independently caching the same underlying data with different TTLs and invalidation logic, causing inconsistent views of the "same" data depending on which service answers.
- A cache is being introduced for the first time and the team needs to decide cache-aside vs. write-through vs. write-behind before writing code, not after a production incident reveals the gap.

## Workflow

1. **Identify what's actually being cached and how often it changes.** Classify data as immutable (safe to cache indefinitely with a version key), slow-changing (cache with TTL + explicit invalidation), or fast-changing/personalized (usually not a good caching candidate, or only cacheable for seconds).
2. **Choose the caching pattern per data type.** Cache-aside (app checks cache, falls back to DB, populates cache) is the simplest and most common default. Write-through (writes go to cache and DB together) keeps cache fresher at the cost of write latency. Write-behind (writes go to cache, async-flushed to DB) maximizes write throughput but risks data loss on cache failure — use only when that risk is acceptable.
3. **Decide the invalidation strategy explicitly per cache, not just a default TTL.** Options: TTL-only (simple, but stale-until-expiry by design), explicit invalidation on write (accurate, but requires every writer to know about the cache), and event-driven invalidation (a write publishes an event that all cache holders subscribe to — scales better across services, adds infra complexity). State which one applies to which cached entity.
4. **Design cache keys to avoid collisions and enable targeted invalidation** — include version, tenant ID, and locale where relevant (`user:v2:{tenantId}:{userId}`), so a schema change or a single tenant's data fix doesn't require flushing the entire cache.
5. **Place caching at the right tier(s)**: CDN/edge for static or public, cacheable-by-URL content; application-level (Redis/Memcached) for computed or personalized-but-shared data; client-side (HTTP cache headers, local storage) for reducing round trips entirely. Most systems need at least two tiers working together, not one cache trying to serve every case.
6. **Plan for cache stampede** on popular keys expiring simultaneously — use jittered TTLs, request coalescing (single in-flight fetch per key, other requests wait on it), or a "soft" expiry that serves slightly stale data while one request refreshes in the background.
7. **Design graceful degradation on cache failure.** The system must survive the cache being unavailable — falling back to the source of truth at higher latency — never treat the cache as a required dependency whose outage takes down the whole read path.
8. **Size the cache and set an eviction policy deliberately** (LRU, LFU, or TTL-driven) based on working-set size and hit-rate targets, not by guessing — undersized caches thrash and provide little benefit while adding latency and complexity.
9. **Instrument hit rate, eviction rate, and staleness incidents per cache**, and alert when hit rate drops sharply (often a sign of a key-schema bug or a stampede) — a cache with no visibility is a cache nobody can safely tune or debug.
10. **Document the consistency guarantee each cache provides** ("this cache may serve data up to 30 seconds stale; the write path invalidates on update but a race window exists") so downstream consumers make informed decisions instead of assuming caches are always perfectly fresh (see `data-consistency-model-selection`).

## Example

Scenario: `ShopWave`'s product catalog API serves 50,000 requests/minute, 95% of which are reads of product detail pages that change only when a merchant edits a listing (a few times a day per product).

Design:
- **CDN tier**: product detail JSON responses cached at the edge with `Cache-Control: max-age=60, stale-while-revalidate=300`, keyed by product ID + locale. Handles ~80% of read traffic without reaching the origin.
- **Application tier**: Redis cache-aside for product data not served from CDN (authenticated requests with personalized pricing), key `product:v3:{productId}:{tenantId}`, TTL 5 minutes with jittered expiry (±30s) to avoid stampede on popular products.
- **Invalidation**: merchant edits publish a `ProductUpdated` event; a cache-invalidation consumer deletes the Redis key and issues a CDN purge for that product's path — bringing worst-case staleness from 5 minutes down to under 2 seconds for edits, while still tolerating consumer lag gracefully (falls back to TTL expiry if the event is delayed).
- **Stampede protection**: request coalescing in the app tier ensures only one origin fetch happens per expired key even under concurrent load, with other requests awaiting that in-flight result.
- **Degradation**: if Redis is unreachable, the app falls back to querying PostgreSQL directly with a circuit breaker (see `circuit-breaker-pattern-design`) capping the blast radius, accepting higher latency rather than downtime.

Result: origin database load drops by ~85%, p95 latency for product pages drops from 240ms to 35ms, and merchant edits are visible to customers within 2 seconds instead of up to 5 minutes.

## Common pitfalls

- **No explicit invalidation strategy**, relying purely on TTL and accepting staleness nobody actually signed off on.
- **Caching personalized or fast-changing data with a long TTL**, serving one user another user's near-stale view of "their" data.
- **Cache key collisions across tenants or schema versions**, causing one tenant's fix to silently serve into another tenant's cache, or a schema change to serve old-shaped data to new code.
- **No stampede protection**, causing a thundering herd on the origin every time a popular key expires simultaneously.
- **Treating the cache as a hard dependency**, so a cache outage takes down reads entirely instead of degrading to a higher-latency but correct fallback.
- **No hit-rate or staleness monitoring**, leaving a poorly-tuned or silently-broken cache undetected until users complain.
- **Multiple services caching the same data independently with different TTLs**, producing visibly inconsistent answers depending on which service is asked.

## Output

A caching architecture doc (e.g., `docs/architecture/caching-<domain>.md`) specifying, per cached entity: caching pattern (cache-aside/write-through/write-behind), tier(s), key schema, TTL and invalidation mechanism, stampede protection, degradation behavior on cache failure, and the stated consistency guarantee — feeding into implementation and into dashboards tracking hit rate and staleness incidents.
