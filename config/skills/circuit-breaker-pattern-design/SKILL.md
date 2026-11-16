---
name: circuit-breaker-pattern-design
description: "Protects callers from cascading failures by wrapping calls to unreliable dependencies in a trip/half-open/reset circuit breaker, so one failing downstream service degrades gracefully instead of exhausting caller resources and taking down the whole system. Use when a service calls another service or third-party API that can fail or slow down."
---

# Circuit Breaker Pattern Design

Wrap calls to any dependency that can fail, slow down, or become unavailable in a circuit breaker that stops calling it once failures cross a threshold — protecting the caller's own threads/connections from being exhausted waiting on a dependency that isn't going to respond, and giving the dependency room to recover instead of being hammered by retries.

## When to use this

- A service calls a downstream dependency (internal service or third-party API) that has, in the past, caused a cascading outage when it became slow or unavailable — callers piled up waiting on it until they ran out of threads/connections themselves.
- Timeouts alone aren't enough because even a fast-failing timeout, retried aggressively by every caller, adds up to overwhelming a struggling downstream service exactly when it needs load reduced, not increased.
- A dependency has a known blast radius across multiple upstream callers, and one caller's retry storm during a downstream incident is making that incident worse for everyone else calling the same dependency.
- The system needs to degrade gracefully (serve cached or default data) when a non-critical dependency is down, rather than fail the entire user-facing request.
- A postmortem found that a slow (not fully down) dependency was worse than a hard failure, because nothing detected the degradation and cut it off — slow failures are the classic case circuit breakers are built for.

## Workflow

1. **Identify calls that need a breaker**: any call to a dependency that can be slow, flaky, or become fully unavailable, especially ones with a track record of degrading rather than failing cleanly. Not every internal call needs one — reserve breakers for dependencies whose failure mode has actually caused (or could plausibly cause) resource exhaustion in the caller.
2. **Choose the three states and their transition rules explicitly**: Closed (calls flow normally, failures counted), Open (calls fail immediately without attempting the dependency, after the failure threshold trips), Half-Open (after a cooldown, a limited number of test calls are allowed through to check recovery). State the exact threshold: e.g., "trip to Open after 50% of the last 20 calls failed" or "trip after 5 consecutive timeouts."
3. **Set the failure criteria precisely** — timeout, 5xx response, or a specific exception type count as failures; explicitly decide whether 4xx client errors (which aren't the dependency's fault) should NOT count toward the trip threshold, since counting them would trip the breaker for a problem the breaker can't fix.
4. **Size the cooldown (Open → Half-Open) period based on the dependency's typical recovery time** — too short retries a still-struggling dependency and re-trips immediately; too long keeps the caller degraded longer than necessary after the dependency has actually recovered.
5. **Bound the Half-Open probe traffic** to a small number of test requests, not a full flood — if the first few succeed, close the breaker; if they fail, reopen and extend the cooldown (with backoff) rather than probing continuously.
6. **Design the fallback behavior for the Open state explicitly per call site**: serve stale cached data, return a default/degraded response, queue the request for later, or fail fast with a clear error to the end user — "fail fast" alone is often not acceptable for user-facing flows; decide what degraded-but-useful behavior looks like.
7. **Pair the breaker with a bulkhead** (separate thread/connection pools per dependency) so a breaker tripping for one dependency doesn't mean the caller was already starved of resources by that dependency before the breaker had a chance to trip — a breaker without resource isolation reacts too late.
8. **Scope breakers per dependency instance, not globally per dependency type**, when calling multiple instances/shards/regions of the same downstream — one bad shard shouldn't trip the breaker for all shards if the others are healthy.
9. **Expose breaker state as a first-class observability signal** — dashboards and alerts on breaker state transitions (`payments-api → OPEN`) give on-call immediate, actionable signal about which dependency is the problem, faster than inferring it from generic latency graphs.
10. **Test the breaker's behavior deliberately** (chaos/fault injection — see `chaos-engineering-experiment-design`) rather than trusting it works correctly only from unit tests; verify it actually trips, actually recovers, and that the fallback path is exercised and correct under real failure injection.

## Example

Scenario: `BookIt`'s reservation service calls a third-party `PricingEngine` API for real-time price calculation on every search. During a `PricingEngine` outage last quarter, `BookIt`'s reservation service exhausted its HTTP connection pool waiting on `PricingEngine` timeouts, which cascaded into failed reservations entirely unrelated to pricing.

Design:
- **Breaker scope**: one breaker instance per `PricingEngine` region endpoint (`us-east`, `eu-west`), so a regional `PricingEngine` outage doesn't trip calls to the healthy region.
- **Trip threshold**: Open after 50% failure rate over the last 20 calls, or 10 consecutive timeouts, whichever comes first. 4xx responses (invalid search parameters) explicitly excluded from the failure count.
- **Cooldown**: 30 seconds, based on `PricingEngine`'s historical mean-time-to-recovery from partial outages (measured from past incidents).
- **Half-Open probe**: 3 test calls; 2/3 success closes the breaker, any failure reopens it with cooldown extended to 60s (capped at 5 minutes on repeated reopens).
- **Fallback**: on Open, `BookIt` serves the last successfully cached price for that route (cached for up to 15 minutes) with a "price may have changed" UI notice, instead of failing the search entirely.
- **Bulkhead**: `PricingEngine` calls use a dedicated 20-connection pool, separate from the pool used for the `InventoryService` call in the same request path, so pricing exhaustion can't starve inventory checks.
- **Observability**: breaker state changes emit a metric and page on-call if Open for longer than 5 minutes.

Result: the next `PricingEngine` degradation trips the breaker within 15 seconds, reservations continue at the cached price with a clear notice, and the reservation service's own resources stay healthy throughout the incident — verified afterward via a chaos experiment that artificially slowed `PricingEngine` responses.

## Common pitfalls

- **Timeouts without a breaker**, so every caller keeps retrying a struggling dependency, adding load exactly when it needs relief.
- **Counting 4xx client errors toward the trip threshold**, tripping the breaker for a problem retrying (or backing off) can't fix.
- **No bulkhead alongside the breaker**, so the caller's resources are already exhausted by the time the breaker's failure count catches up and trips.
- **Half-Open state flooding the dependency with a full traffic retry** instead of a small bounded probe, re-triggering the outage the breaker just detected.
- **One global breaker across multiple instances/shards of a dependency**, so a single bad shard needlessly cuts off calls to healthy shards too.
- **No defined fallback for the Open state**, leaving the caller to fail in whatever way happens to fall out of the code path, rather than a deliberate degraded response.
- **Never testing the breaker under real fault injection**, so its first real trip happens during an actual incident instead of during a planned chaos exercise.

## Output

A circuit breaker design note per protected dependency (e.g., `docs/architecture/circuit-breakers.md`) specifying: scope (per instance/shard), failure criteria and trip threshold, cooldown and half-open probe sizing, fallback behavior per call site, paired bulkhead sizing, and the observability/alerting hooked to state transitions — validated with a fault-injection test before relying on it in production.
