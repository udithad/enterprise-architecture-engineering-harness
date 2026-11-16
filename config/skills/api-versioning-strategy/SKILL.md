---
name: api-versioning-strategy
description: "Chooses a versioning scheme (URI path, custom header, or media-type/content-negotiation) and a deprecation policy for an API, so evolving the contract doesn't break existing consumers on release day. Use when adding a breaking change to a public or partner-facing API, when multiple client versions must be supported simultaneously, or when a team has no documented rule for when a change requires a new version."
---

# API Versioning Strategy

Version the contract, not the implementation: a versioning scheme exists to let you change what an API promises without breaking who already depends on the old promise, and every scheme is a trade-off between how visible the version is to callers and how much routing complexity it pushes onto your infrastructure — pick one on purpose, and pair it with a deprecation policy that has actual teeth (dates, telemetry, and a shutoff), not just a comment in the changelog.

## When to use this

- A breaking change (removed field, changed type, renamed endpoint, altered semantics) is needed on an API that already has external or cross-team consumers.
- Two or more client versions (a mobile app that can't force-update, a partner integration, an internal service on an older contract) must be served correctly at the same time.
- A team has been bumping a version number inconsistently — sometimes for breaking changes, sometimes for additive ones — and consumers no longer trust what a version bump signals.
- A postmortem traces an outage to a client silently breaking because a field was removed or renamed without warning.
- An API is going public (or gaining its first external partner) and needs a documented policy before, not after, the first deprecation.
- A GraphQL or gRPC API is being evaluated and the team assumes "we don't need versioning because it's not REST" (see `grpc-vs-rest-vs-graphql-decision`).

## Workflow

1. **Define what counts as breaking before choosing a scheme.** Adding an optional field, adding a new endpoint, or adding a new enum value consumers are contractually required to ignore-unknown-values on is non-breaking. Removing/renaming a field, tightening validation, changing a field's type or units, or changing default behavior is breaking. Write this list down — it's the actual policy; the scheme is just the delivery mechanism.
2. **Pick the versioning location deliberately, not by convention.** URI path (`/v2/orders`) is the most visible and cacheable-by-URL, but conflates resource identity with version and tends to freeze the whole API at once. A custom header (`API-Version: 2024-06-01`) or media-type negotiation (`Accept: application/vnd.acme.order+json;version=2`) allows per-resource or per-field versioning and keeps URLs stable, but is invisible in logs/browsers unless you actively surface it, and is easy for consumers to omit and get silently defaulted. Date-based versions (Stripe-style `2024-06-01` header) decouple "a version" from "a big-bang release" and suit APIs that evolve continuously.
3. **Decide the granularity: whole-API version vs. per-resource/per-field version.** A single global version (most REST APIs) is simpler to reason about and document but forces clients to jump every breaking change at once, even ones they don't use. Per-resource versioning is more work to implement and document but lets a consumer upgrade incrementally.
4. **Design the negotiation and default-version fallback explicitly.** Decide what happens when a client sends no version header at all: default to the latest (breaks old integrations that predate the header being required), default to the oldest supported (safe but means "new" is opt-in forever), or reject with `400` (safest for a new API, hostile for an established one). Document this choice; it is the single most consequential default in the whole scheme.
5. **Version the schema and the transport together.** For JSON/REST, pair the version scheme with explicit schema evolution rules (nullable-by-default new fields, no field reuse, no type changes) — see `backward-compatibility-strategy` for the underlying contract-change discipline this versioning scheme is delivering.
6. **For gRPC, lean on protobuf's built-in evolution instead of bolting on a version header.** Field numbers are permanent and additive; deprecate fields with `[deprecated = true]` and reserve their numbers (`reserved 7, 9;`) rather than reusing them. Only stand up a new service version (`OrderServiceV2`) when the change is genuinely incompatible at the RPC-signature level, not for every field tweak.
7. **For GraphQL, version fields, not the whole schema.** Add new fields alongside old ones, mark deprecated fields with `@deprecated(reason: "...")`, and drive removal off resolver-level usage analytics (which clients still query the deprecated field) rather than a schema-wide cutover.
8. **Set a deprecation policy with concrete dates and enforcement, not just a note.** Publish: the sunset date, a migration guide, and what happens at sunset (hard `410 Gone`, or a grace period with degraded/rate-limited service). Put the sunset date in a machine-readable response header (`Sunset:`, `Deprecation:` per RFC 8594) so clients and monitoring can react automatically, not just humans reading docs.
9. **Instrument version usage before deprecating anything.** Log the version/media-type on every request and build a dashboard of traffic per version. Never announce a sunset date for a version you don't have usage data for — you can't tell if it's safe to remove.
10. **Give consumers a proactive off-ramp, not just a deadline.** Email or dashboard-notify accounts still calling a soon-to-be-retired version, ideally driven by the same usage telemetry from step 9, well before the sunset date — a deadline nobody who's affected knows about isn't a policy.
11. **Cap the number of simultaneously supported versions.** Pick a number (commonly 2–3) and enforce it: shipping `v4` obligates retiring `v1` on a schedule, not "eventually." Unbounded version sprawl is how a team ends up maintaining five parallel code paths indefinitely.
12. **Cross-check against idempotency and compatibility work already in flight.** A version bump changes the request/response shape an idempotency dedup record was created under — make sure retried requests across a version boundary still resolve sanely (see `idempotency-design-for-apis`), and that the change itself follows the compatibility rules in `backward-compatibility-strategy`.

## Example

Scenario: Acme Payments exposes a public REST API used by ~400 partner integrations. The team needs to change `POST /v1/charges` so `amount` moves from an integer (cents) to a structured `{value, currency}` object — a breaking type change.

Chosen scheme: date-based version header, modeled on Stripe.

```
Request header:  API-Version: 2024-06-01
Response header: API-Version: 2024-06-01
                 Deprecation: true                (only on deprecated versions)
                 Sunset: Sat, 01 Mar 2025 00:00:00 GMT
```

Rollout table:

| Date | Event |
|---|---|
| 2024-06-01 | New version released; `amount` becomes `{value, currency}`. Old integer format still served to any request whose `API-Version` header is `2023-01-01` or omitted (default = oldest, per step 4). |
| 2024-06-01 → 2024-12-01 | Usage dashboard tracks per-partner version; Solutions team emails the 40 partners still on `2023-01-01` with a migration guide. |
| 2024-12-01 | `Deprecation: true` and `Sunset` headers added to all responses served under `2023-01-01`. |
| 2025-03-01 | `2023-01-01` requests start receiving `410 Gone` with a body pointing to the migration guide; 9 remaining partners had been individually contacted by account management in the final 30 days. |

Rejected alternatives:

| Alternative | Rejected because |
|---|---|
| URI path version (`/v2/charges`) | Would require every partner integration to change every call site's URL for a single field's type change; the header approach lets a partner adopt the new version by changing one config value |
| Bump version and force all clients to migrate on a fixed date with no header-based coexistence | 400 partner integrations can't all redeploy simultaneously; a hard cutover would break the majority of partners on day one |
| Silent breaking change with no version bump ("just fix the field") | Discovered only via the postmortem this policy was written to prevent — partners' Stripe-charge parsing code broke in production with zero warning |

## Common pitfalls

- **Bumping the version for non-breaking changes.** Trains consumers to ignore version numbers and to over-pin to an exact version out of learned distrust.
- **No documented default when the version is omitted.** Silently changes behavior for every integration that predates the versioning scheme existing at all.
- **Announcing a sunset date with no usage telemetry backing it.** You cannot know if a "low-traffic" version is actually a critical partner's only integration.
- **Freezing the whole API on every version bump (URI-path global versioning) when only one field changed.** Forces consumers to re-certify against an entirely new contract for an unrelated change.
- **Treating `deprecated` as documentation-only instead of a machine-readable, monitorable signal.** Without `Deprecation`/`Sunset` headers or equivalent telemetry, nobody — client or server team — has an automated trigger to act before the deadline.
- **Unbounded concurrently-supported versions.** Each additional live version is a permanent tax on every future change; without an enforced cap, old versions never get retired.
- **Reusing a protobuf field number or JSON field name for a different meaning "since the old one wasn't used much."** Breaks any client still holding a cached schema or generated stub, and is nearly impossible to detect in code review.

## Output

A versioning policy document (commonly `docs/architecture/api-versioning.md` or a section of the API's OpenAPI/proto spec) stating: the chosen version location and granularity, the breaking-vs-non-breaking change classification, the default-version-on-omission behavior, the maximum number of concurrently supported versions, and the deprecation/sunset process including headers and notification triggers — feeding directly into the API's public documentation, its CI contract-diff check, and the schema-evolution rules tracked in `backward-compatibility-strategy`.
