---
name: grpc-vs-rest-vs-graphql-decision
description: "Picks the right API style — gRPC, REST, or GraphQL — per use case based on latency needs, client diversity, payload shape flexibility, and team/tooling maturity, instead of defaulting to whichever style is currently fashionable. Use when designing a new API or when an existing API style is causing friction for its actual consumers."
---

# gRPC vs. REST vs. GraphQL Decision

Choose an API style based on who's calling it and what they need — internal service-to-service calls needing low latency and strict contracts, public/partner APIs needing broad compatibility and cacheability, or diverse clients needing flexible, precisely-shaped queries — instead of picking one style for every API in the organization by default.

## When to use this

- A new API is being designed and the team is debating REST vs. GraphQL vs. gRPC without a structured way to decide.
- An existing REST API is causing over-fetching/under-fetching pain for a client (e.g., a mobile app making 6 calls to assemble one screen, or downloading a bloated response for 3 needed fields).
- Internal service-to-service calls are using REST/JSON and hitting latency or serialization-overhead limits that a binary protocol could address.
- A public/partner API needs broad client compatibility, caching via standard HTTP semantics, and easy debuggability with common tooling — properties that come naturally to REST but need extra work in gRPC or GraphQL.
- Multiple client types (web, mobile, partner integrations) each want a different shape of the same data and the team is deciding whether GraphQL's flexible querying is worth its added complexity versus a BFF-per-client REST approach (see `api-gateway-design`).

## Workflow

1. **Identify the caller type first**: internal service-to-service, first-party client (your own web/mobile apps), or third-party/public consumers — this alone eliminates options. Third-party/public APIs strongly favor REST (universal tooling, cacheable, easy to document and debug with curl). Internal service-to-service strongly favors gRPC where latency and strict typed contracts matter.
2. **Evaluate latency and serialization overhead requirements.** gRPC's HTTP/2 multiplexing and Protobuf binary serialization meaningfully beat REST/JSON for high-throughput, low-latency internal calls (see `api-latency-reduction-strategy`) — if this isn't actually a bottleneck, the added tooling/debugging complexity of gRPC may not be worth it.
3. **Evaluate payload shape diversity across clients.** If different clients need meaningfully different subsets/shapes of the same underlying data (a mobile home screen needing 5 fields from 3 entities vs. a web dashboard needing everything), GraphQL's client-specified queries avoid both over-fetching and the need to hand-build a BFF endpoint per client shape — but this benefit shrinks if you already have (or are willing to build) BFFs.
4. **Weigh caching needs.** REST's use of standard HTTP semantics (GET + Cache-Control, ETags) gets you CDN and browser caching essentially for free. GraphQL (typically single POST endpoint) and gRPC (binary streams) need bespoke caching strategies — a real cost if cacheability at the HTTP layer matters for this API's traffic profile.
5. **Weigh streaming needs.** gRPC has first-class support for client-streaming, server-streaming, and bidirectional streaming (e.g., real-time telemetry, chat) that REST and GraphQL support only with extra layers (WebSockets, Server-Sent Events, GraphQL subscriptions) — if the use case is fundamentally a stream, this tips meaningfully toward gRPC.
6. **Weigh team and ecosystem maturity honestly.** gRPC and GraphQL both carry a real learning curve, tooling investment (schema registries, codegen pipelines, gateway/federation infra for GraphQL), and debugging-ergonomics cost compared to REST's ubiquity — factor in whether the team already has this expertise or would be building it from scratch for one API.
7. **Consider a mixed approach across layers, which is common and often correct**: gRPC for internal service-to-service calls, REST or GraphQL at the public/BFF edge — see `api-gateway-design` for the translation layer, since clients don't need to know or care what protocol internal services use to talk to each other.
8. **Assess versioning and evolution needs** (`api-versioning-strategy`). Protobuf's field-numbering scheme gives gRPC strong backward-compatible evolution by design; REST needs an explicit versioning strategy; GraphQL's schema can evolve additively (deprecate fields rather than remove) but requires discipline to avoid breaking clients on a shared schema.
9. **Evaluate strict typing and contract enforcement needs.** gRPC's Protobuf-defined contracts and GraphQL's schema both give compile-time/query-time type safety that plain REST/JSON (without an OpenAPI-generated client) doesn't enforce by default — if contract drift between client and server has caused real incidents, this favors gRPC or GraphQL over unstructured REST.
10. **Decide, document as an ADR, and don't force organization-wide uniformity for its own sake** — the right choice is genuinely different for a public partner API, an internal high-throughput service call, and a diverse-multi-client BFF; forcing one style everywhere trades away real fit for the sake of consistency that doesn't actually help anyone.

## Example

Scenario: `MetroTransit`'s new trip-planning platform has three API consumer types: (1) internal microservices computing routes, fares, and real-time vehicle positions that call each other constantly and need low latency; (2) the company's own iOS/Android/web apps, each wanting a different shaped view of trip data; (3) third-party developers building transit apps against a public API.

Decisions:
- **Internal service-to-service (routing ↔ fares ↔ vehicle-position services)**: gRPC. High call volume (thousands/sec internally), strict typed contracts prevent the kind of schema-drift incident that happened last year, and server-streaming is a natural fit for the vehicle-position service pushing live updates to the routing service.
- **First-party client-facing API (iOS/Android/web)**: GraphQL behind a BFF gateway. The three clients need meaningfully different shapes of trip/fare/vehicle data (mobile wants a minimal home-screen payload, web wants a fuller dashboard view), and GraphQL avoids hand-building three separate REST BFF endpoints while giving each client precise control over payload shape.
- **Public/partner API**: REST. Third-party developers need standard tooling, HTTP caching for popular routes (cached via CDN with `Cache-Control`), easy debugging with curl/Postman, and broad language/tooling support without requiring a GraphQL or gRPC client library.
- **Translation**: the API gateway (`api-gateway-design`) translates the public REST API's requests into internal gRPC calls; clients never see or care that the backend uses gRPC internally.

Result: internal latency for a full trip-planning call drops from ~180ms (REST/JSON) to ~60ms (gRPC/Protobuf); the mobile app's home-screen load drops from 5 round trips to 1 GraphQL query; the public API remains the broadly-compatible, cacheable, easy-to-debug REST surface partners expect.

## Common pitfalls

- **Picking one API style organization-wide "for consistency"** and forcing it onto use cases (public APIs, streaming workloads) it fits poorly.
- **Adopting GraphQL for a single-client API** where the flexible-query benefit doesn't materialize, but the schema/resolver/caching complexity cost is paid anyway.
- **Adopting gRPC for a public/partner API**, creating unnecessary friction for third-party developers who expect REST's ubiquitous tooling and HTTP-native caching.
- **Underestimating GraphQL's operational complexity** — N+1 query problems at the resolver level, query-cost/depth limiting to prevent abusive queries, and cache-strategy design are all real, non-optional work.
- **Underestimating gRPC's debugging-ergonomics cost** for teams without existing Protobuf/gRPC tooling — binary payloads aren't curl-able the way JSON is, and this friction is real during incident response.
- **No versioning/evolution strategy chosen up front**, leading to breaking changes for whichever style was picked once the schema/contract needs its first real revision.

## Output

An API style decision doc or ADR (e.g., `docs/architecture/api-style-<surface>.md`) per API surface (internal, first-party client-facing, public/partner) stating the chosen style, the caller-type and latency/caching/streaming reasoning behind it, the versioning/evolution approach, and how translation between styles happens at any gateway boundary.

