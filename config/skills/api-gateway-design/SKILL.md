---
name: api-gateway-design
description: "Designs the single entry point that fronts a set of backend services — routing, authn/authz, rate limiting, request aggregation, and protocol translation — so clients don't need to know service topology. Use when multiple client types (web, mobile, partner) each need a different shape of the same backend, or when cross-cutting concerns are being duplicated across every service."
---

# API Gateway Design

Put one well-designed layer between external clients and internal services that handles routing, authentication, rate limiting, and response shaping centrally, instead of every service re-implementing the same cross-cutting concerns or every client hard-coding knowledge of service topology.

## When to use this

- Clients currently need to know the network address of every backend service directly, and that list changes often enough that client releases keep breaking.
- Authentication, rate limiting, request logging, or TLS termination are duplicated (and drifting out of sync) across a dozen services instead of enforced in one place.
- Different client types (public web app, native mobile, partner API consumers) need meaningfully different shapes of the same underlying data, and today each service tries to serve all of them with one bloated response format.
- A single logical client action requires calling 3+ backend services, and you want to move that fan-out/aggregation off the client (especially a mobile client on a slow network) onto the server side.
- The organization needs a single place to enforce API versioning policy, apply a consistent rate-limit tier per API key, or produce unified access logs/metrics across every service, and today that's scattered or missing.
- A migration is underway (see `monolith-to-microservices-migration-plan`) and the gateway needs to route some paths to the legacy monolith and others to new services transparently, so clients don't notice the migration happening underneath them.

## Workflow

1. **Decide gateway scope: edge gateway vs. Backend-for-Frontend (BFF) vs. both.** A single edge gateway in front of everything is simpler to operate but risks becoming a bloated one-size-fits-all layer trying to serve web, mobile, and partner clients identically. A BFF per client type (web-BFF, mobile-BFF, partner-BFF) lets each shape responses for its client, sitting behind a thinner shared edge gateway that handles only truly universal concerns (TLS, top-level auth, DDoS protection). Choose BFFs when client needs have genuinely diverged; skip them if one response shape serves everyone fine.
2. **Enumerate the cross-cutting concerns the gateway will centralize** — TLS termination, authentication (token validation), coarse-grained authorization, rate limiting (see `rate-limiting-strategy-design`), request/response logging, and API versioning enforcement (see `api-versioning-strategy`). Be explicit about what stays in the gateway vs. what remains each service's own responsibility (fine-grained, domain-specific authorization almost always belongs in the service, not the gateway).
3. **Design the routing rules explicitly as versioned configuration**, not ad hoc code — path-based (`/orders/*` → Order Service), header-based, or client-type-based routing rules should be reviewable and testable independently of any single service's deploy.
4. **Decide on request aggregation/composition carefully.** A gateway-level aggregation endpoint that calls 3 services and merges results is convenient for mobile clients, but it moves business logic (how to merge, what to do on partial failure) into the gateway — keep aggregation logic thin and orchestration-free; if it grows real business rules, that's a sign it belongs in a proper BFF or an orchestrating service instead.
5. **Plan for partial backend failure in every aggregated call.** If a gateway endpoint calls 3 services and one times out, decide explicitly: fail the whole request, return partial data with a flag, or serve a cached/fallback value — don't let this be accidental (see `circuit-breaker-pattern-design` for per-backend isolation).
6. **Keep the gateway stateless and horizontally scalable.** Session state, rate-limit counters, and auth caches belong in a shared store (Redis) the gateway reads/writes, not in gateway-instance memory — otherwise scaling the gateway breaks sticky assumptions.
7. **Avoid putting business logic in the gateway.** Domain validation, workflow state transitions, and business rules belong in services; a gateway that accumulates business logic becomes a second monolith that every team must coordinate through, defeating the purpose of the service boundaries underneath it.
8. **Design authentication/authorization propagation to backends.** Decide how identity flows downstream after the gateway validates it — a signed internal JWT, a trusted header set (only safe inside a locked-down network), or mTLS-based service identity (see `service-mesh-evaluation` if this is being handled at the infrastructure layer instead of the gateway layer).
9. **Plan the gateway's own high availability and deployment strategy.** Because every request flows through it, the gateway is a single point of failure by construction — run it multi-instance behind a load balancer, in multiple availability zones, and treat its deploys with the same zero-downtime rigor as any tier-0 service (see `zero-downtime-deployment-architecture`, `high-availability-architecture-design`).
10. **Instrument the gateway as the primary observability chokepoint** — since every external request passes through it, it's the natural place to emit consistent request tracing, latency, and error-rate metrics per route (see `observability-architecture-design`).
11. **Decide gateway vs. service mesh division of labor explicitly** if both exist: the gateway handles north-south (client-to-system) traffic concerns; a service mesh (see `service-mesh-evaluation`) handles east-west (service-to-service) traffic concerns like mTLS and retries between internal services — don't duplicate the same concern in both layers without a reason.

## Example

Scenario: `FreightFlow`, a logistics platform, has a public web dashboard, a driver-facing mobile app, and third-party partner integrations, all currently calling seven backend services (`Shipments`, `Tracking`, `Drivers`, `Billing`, `Routing`, `Documents`, `Notifications`) directly. Each client independently implements auth-token handling, and the mobile app makes 6 sequential calls just to render its home screen, causing an 8-second load time on cellular connections.

Design:

```
Web client   --> Edge Gateway --> Web-BFF   --> [Shipments, Tracking, Billing]
Mobile app   --> Edge Gateway --> Mobile-BFF --> [Shipments, Tracking, Drivers, Routing]  (aggregates 4 calls into 1)
Partner API  --> Edge Gateway --> Partner-BFF --> [Shipments, Documents]  (stable, versioned, rate-limited per partner key)
```

Edge Gateway (shared, thin): TLS termination, JWT validation, top-level rate limiting (1000 req/min per API key), routes to the correct BFF by client-type header.

Mobile-BFF: aggregates `Shipments.getActive`, `Tracking.getLatestLocation`, `Drivers.getProfile`, and `Routing.getNextStop` into one `GET /mobile/home` response — reduces mobile home-screen load from 6 round trips to 1, cutting load time from 8s to under 1.5s on the same network. On `Routing` timeout, Mobile-BFF returns the other 3 results with `routing: null` and a `partial: true` flag rather than failing the whole request (Drivers care more about seeing shipment status than the next stop).

Rejected alternative: one shared BFF for all three client types. Rejected because the partner API needs a stable, narrowly-scoped, versioned contract (partners can't tolerate a mobile-driven schema change breaking their integration), while the mobile BFF needs to change frequently to match app releases — coupling their release cadence via one shared BFF would force partner-breaking caution onto every mobile-driven change.

Rate limiting: partner API keys get a strict 100 req/min tier enforced at the edge gateway; internal web/mobile clients authenticate via user session tokens and are rate-limited per-user instead of per-key.

## Common pitfalls

- **Business logic creeping into the gateway.** Once a team adds "just one small" conditional business rule to a gateway route, it becomes a magnet for more, turning the gateway into an unversioned, hard-to-test shadow service.
- **One shared BFF serving fundamentally different client needs.** Forces unrelated clients (e.g., a slow-changing partner API and a fast-iterating mobile app) onto the same release cadence and schema.
- **Making the gateway a stateful bottleneck.** Storing session or rate-limit state in gateway-instance memory prevents clean horizontal scaling and causes inconsistent behavior across instances.
- **No plan for partial backend failure in aggregated calls.** An aggregation endpoint that fails the entire request because one of five backends timed out turns a minor backend hiccup into a full outage for every client.
- **Treating the gateway as a substitute for per-service authorization.** Coarse-grained gateway auth ("is this a valid token") is not a substitute for the service enforcing its own fine-grained business authorization rules ("can this specific user cancel this specific shipment").
- **Underestimating the gateway's own availability requirements.** Every client request depends on gateway uptime; treating it as a lower-priority deploy target than the services behind it inverts the actual criticality.
- **Duplicating service-mesh concerns at the gateway (or vice versa) without a reason.** Handling retries/mTLS at both the gateway and the mesh for the same traffic adds operational confusion about which layer is actually enforcing what.

## Output

An API gateway design document (e.g., `docs/architecture/api-gateway-design.md`) specifying: gateway topology (single edge gateway, BFF-per-client, or both), the explicit list of cross-cutting concerns centralized at each layer, routing rules per client type, aggregation endpoints with their partial-failure behavior, and the gateway's own HA/deployment plan — feeding into ADRs and into the gateway's routing configuration.
