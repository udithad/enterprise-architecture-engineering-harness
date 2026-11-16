---
name: rate-limiting-strategy-design
description: "Designs a rate-limiting strategy for an API or service — which algorithm to use, where to enforce it, what dimensions to limit on, and how to make it correct under distributed load. Use when a service is being abused, needs tiered quotas, or must protect a downstream dependency from too much inbound traffic."
---

# Rate Limiting Strategy Design

Decide how, where, and on what dimensions to throttle inbound requests so a service stays available under load, quota tiers are enforced fairly, and the limiter itself doesn't become a single point of failure or a source of subtle race conditions.

## When to use this

- A public or partner-facing API needs tiered quotas (free/pro/enterprise) enforced per API key or per customer.
- A service was taken down by a traffic spike — legitimate or a scraping/credential-stuffing attack — and needs proactive throttling, not just reactive scaling.
- A service calls a downstream with a hard quota (a payment gateway, a third-party data provider) and must self-throttle to stay under that quota rather than get cut off.
- Multiple services are each rolling their own ad hoc counters and limits are inconsistent or incorrect across replicas.
- Someone conflates rate limiting with circuit breaking and proposes one mechanism to solve both problems.
- A load test reveals that the current limiter under-counts or over-counts requests when traffic is split across multiple nodes.

## Workflow

1. **Name what you're protecting.** A rate limit either protects your own service from overload, protects a shared downstream dependency from too much aggregate traffic, or enforces a commercial quota. Each of these implies different placement and different failure behavior — conflating them produces a limiter tuned for the wrong thing.
2. **Choose the algorithm based on the traffic shape you want to allow.** Fixed-window counters are cheap but allow up to 2x burst at window boundaries (two windows' worth of requests can land back-to-back at the boundary). Sliding-window log/counter is accurate but costs more memory (a log of timestamps, or a weighted count across two windows). Token bucket allows controlled bursts up to bucket capacity while enforcing a steady average rate — the usual default for API traffic. Leaky bucket smooths bursts into a constant output rate — better when the downstream truly cannot tolerate any burst (e.g., a fixed-capacity legacy system).
3. **Decide the enforcement point(s).** Enforce coarse, cheap limits at the edge/API gateway (see `api-gateway-design`) to shed obviously-abusive traffic before it costs anything. Enforce fine-grained, business-aware limits (per-tenant quota, per-endpoint cost weighting) at the service layer, since the gateway usually doesn't know business context. Never rely on client-side throttling alone — it's a courtesy for well-behaved clients, not a control.
4. **Pick the limiting dimensions and how they compose.** Common axes: per-API-key, per-user, per-IP (for unauthenticated endpoints), per-tenant, and global (protecting the whole system or a shared downstream). Decide explicitly whether dimensions are independent limits (any one tripping rejects the request) or nested (a per-user limit inside a per-tenant ceiling) — undocumented interaction between axes is a common source of "why was I limited, I'm under my quota" support tickets.
5. **Solve distributed correctness before picking a data store.** A naive `GET counter; if counter < limit: INCR` from multiple app nodes race — two nodes can both read the same under-limit value and both admit a request that together exceeds the limit. Use an atomic operation: a Lua script in Redis (read-check-increment as one atomic step), Redis's `INCR`+`EXPIRE` with careful ordering, or a token-bucket implementation stored in Redis with atomic decrement. For very high-throughput edge limiting, an approximate/local-then-sync approach (each node keeps a local budget and reconciles periodically) trades strict accuracy for avoiding a shared-store bottleneck — decide if that approximation is acceptable for the use case.
6. **Design the client contract.** Return `429 Too Many Requests` with a `Retry-After` header and `X-RateLimit-Limit` / `X-RateLimit-Remaining` / `X-RateLimit-Reset` headers so well-behaved clients can back off correctly instead of hammering retry. Document what counts against the quota (failed requests too, or only successful ones) — this is a frequent source of disputes with API consumers.
7. **Decide degradation behavior, not just rejection.** For some endpoints, a soft response (serve stale cached data, degrade to a cheaper code path) beats a hard 429 — see `caching-layer-architecture` for what can be served from cache under load. For others (write endpoints, payment calls) hard rejection is correct because a degraded response would be wrong, not just slow.
8. **Size limits from real capacity, not round numbers.** Derive the limit from what the protected resource (a database connection pool, a downstream quota, a CPU budget) can actually sustain — see `capacity-planning-and-scaling-model` — then leave headroom, don't set the limit at the resource's exact breaking point.
9. **Separate this from circuit breaking explicitly.** Rate limiting is proactive: it caps inbound traffic regardless of whether the callee is healthy. Circuit breaking (see `circuit-breaker-pattern-design`) is reactive: it trips only when a downstream is actually failing or slow. A service typically needs both — a rate limiter on inbound requests, and a circuit breaker on its own outbound calls to dependencies — and they should not be implemented as the same mechanism.
10. **Instrument before tuning.** Ship dashboards on rejection rate per dimension, and alert when a legitimate customer is consistently hitting their ceiling (a signal to review their tier, not just a security event). Tune thresholds from this data, not guesses, and revisit them as traffic patterns shift.
11. **Account for cost, not just capacity.** If the limiter itself calls out to a shared store (Redis) on every request, that adds latency and cost at scale — see `cost-aware-architecture-design`. Weigh a slightly less precise but cheaper local-approximation scheme against a fully centralized, precise one for very high QPS endpoints.

## Example

Scenario: `PaySwift`, a payments API product, offers three tiers (Free, Pro, Enterprise) and also calls a downstream card-network gateway with a hard contractual cap of 500 requests/sec aggregate.

Enforcement layers in the request path:

```
Client --> Edge/API Gateway (per-IP, per-API-key limits, cheap+coarse)
             |  token bucket, Redis-backed, Lua script for atomic check+decrement
             v
       PaySwift Charge Service (per-tenant quota by tier; per-endpoint cost weighting)
             |
             v
       Downstream Gateway Guard (global token bucket, 500 req/s ceiling, shared across all tenants)
```

Tier limits (token bucket, refill continuous):

| Tier | Sustained rate | Burst capacity | Enforced at |
|---|---|---|---|
| Free | 5 req/s | 10 tokens | API key, edge |
| Pro | 50 req/s | 150 tokens | API key, edge |
| Enterprise | 300 req/s | 600 tokens | API key, edge + dedicated downstream sub-budget |

The downstream gateway guard is a **separate** global token bucket capped at 500 req/s total, refilled continuously, shared across all tenants — because the 500 req/s ceiling is a contractual limit on the whole system, not a per-tenant concern. Enterprise tenants are additionally given a reserved sub-allocation (e.g., 120 req/s carved out of the 500) so one large Free-tier traffic burst can't starve a paying Enterprise customer's downstream calls.

Rejected alternatives:

| Alternative | Rejected because |
|---|---|
| Fixed-window counter (1-second windows) at the edge | Allowed up to 2x burst at window boundaries — two adjacent 5 req/s windows could deliver 10 requests in 200ms, which was enough to occasionally trip the downstream gateway's own burst protection |
| Single shared limit dimension (per-API-key only, no separate downstream guard) | A Pro-tier customer alone, staying under their own per-key limit, could still push the aggregate over the 500 req/s downstream contract if enough Pro customers synced traffic; needed the separate global guard |
| Client-side throttling only (rely on SDK to pace requests) | Several third-party integrations bypassed the official SDK entirely; client-side throttling is a courtesy, not a control |

On rejection, the API returns `429` with `Retry-After: 2`, `X-RateLimit-Limit: 50`, `X-RateLimit-Remaining: 0`, `X-RateLimit-Reset: <unix-ts>`. Charge-creation endpoints (state-changing) always hard-reject on limit breach; read-only balance-check endpoints instead serve a cached balance up to 30 seconds stale rather than 429ing, per the caching-layer-architecture policy for that endpoint.

## Common pitfalls

- **Naive check-then-increment across distributed nodes.** Without an atomic operation, two nodes reading the same under-limit counter both admit a request, silently doubling the effective limit under concurrency.
- **One global limiter for unrelated concerns.** Mixing "protect my service from abuse" and "respect the downstream's contractual quota" in a single limit means tuning one breaks the other.
- **Conflating rate limiting with circuit breaking.** A rate limiter caps volume regardless of downstream health; a circuit breaker responds to downstream failure. Using one to do the other's job leaves a gap (traffic within the limit still hammers a dying downstream) or overreaction (healthy downstream gets throttled because inbound volume alone tripped a "breaker").
- **Fixed-window boundary bursts.** A limit expressed as "N per second" using naive fixed windows silently permits up to 2N in a short window around the boundary — invisible until a load test catches it.
- **No visible feedback to well-behaved clients.** Returning a bare `429` with no `Retry-After` or remaining-quota headers turns every client into a blind-retry client, worsening the load the limiter exists to control.
- **Setting limits at the resource's exact breaking point.** Zero headroom means normal variance in downstream latency or load tips the system over; size limits with margin, informed by `capacity-planning-and-scaling-model`.
- **Treating client-side throttling as sufficient.** Any SDK-level pacing is advisory; third-party or malicious clients will bypass it. Server-side enforcement is the only actual control.

## Output

A rate-limiting design document (e.g., `docs/architecture/rate-limiting-strategy.md`) specifying: the algorithm and enforcement point per traffic class, the limiting dimensions and how they compose, the atomic-correctness mechanism for the distributed counter store, the client-facing contract (status code, headers, retry guidance), degradation vs hard-rejection policy per endpoint, and the numeric limits with their derivation from downstream/resource capacity — feeding into gateway configuration and service-level middleware implementation.
