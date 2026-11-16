---
name: idempotency-design-for-apis
description: "Designs APIs and message handlers so retries and duplicate deliveries produce the same result instead of duplicate side effects, using idempotency keys and a dedup store where operations aren't naturally idempotent. Use when a client-facing write endpoint, payment flow, or message consumer must be safe under at-least-once delivery."
---

# Idempotency Design for APIs

Make retries safe by guaranteeing a repeated request produces the same effect and returns the same result as the original — not by hoping clients never retry, and not by treating "idempotent" as a synonym for "safe" or "atomic," which it isn't.

## When to use this

- A client-facing endpoint creates a resource with side effects (charge a card, place an order, send a payment) and network timeouts mean the client can't tell whether the first attempt succeeded.
- A message consumer reads from a queue or stream with at-least-once delivery and must not double-process a redelivered message.
- A saga step (see `saga-pattern-for-distributed-transactions`) will be retried by the orchestrator on failure and must not re-apply its effect if it already partially succeeded.
- An incident review finds a customer was double-charged, a duplicate order was created, or a duplicate email was sent because of a retried request.
- A team is designing a new write API and wants to decide up front which operations are idempotent by construction and which need an explicit idempotency-key mechanism.

## Workflow

1. **Separate "idempotent" from "safe" and "atomic."** Idempotency guarantees that repeating an operation produces the same end state and the same response as the first successful call — it does not by itself guarantee the operation is atomic, consistent with concurrent operations, or free of partial-failure states. Design each of those properties deliberately; don't assume idempotency subsumes them.
2. **Classify each write operation as idempotent-by-construction or not.** `PUT` with a full resource replacement is naturally idempotent (applying it twice yields the same state). Conditional writes keyed on a version or `ETag` (`If-Match`) are idempotent because a stale write is rejected rather than reapplied. An upsert keyed on a natural business key (e.g., `(order_id, line_item_sku)`) is idempotent because re-running it converges to the same row. A `POST` that creates a new resource with a server-generated ID is **not** idempotent by construction — every call creates a new one — and needs an explicit mechanism.
3. **For non-idempotent-by-construction operations, use the idempotency-key pattern.** The client generates a unique key (a UUID) per logical operation attempt and sends it as a header (e.g., `Idempotency-Key`). The server, before executing any side effect, attempts to atomically claim that key in a dedup store. If the claim succeeds, proceed and record the outcome; if the key already exists, do not re-execute — return the previously recorded response instead.
4. **Design the claim step to be atomic, not check-then-act.** A naive "look up the key, if absent then execute" races: two near-simultaneous requests with the same key can both pass the lookup before either writes its result, and both execute. Use a single atomic insert with a uniqueness constraint (e.g., `INSERT ... ON CONFLICT DO NOTHING` returning whether the row was newly created) as the claim, or a distributed lock, so exactly one caller proceeds and the other waits for or reads the first's result.
5. **Persist enough in the dedup store to detect a conflicting reuse of the same key.** Store the key, a hash of the request body, the execution status (in-progress/completed/failed), the response payload, and an expiry. If a retried key arrives with a *different* request body than the one originally associated with it, reject with a conflict (e.g., `409`) rather than silently executing the new body under the old key's identity — reusing a key for a different logical operation is a client bug that must surface, not get silently absorbed.
6. **Choose the dedup store and TTL deliberately.** Redis (fast, TTL-native, but not durable by default) suits short-lived retry windows (minutes to hours); a database table suits longer windows or where the dedup record must survive a cache outage. Set the TTL to comfortably exceed the longest realistic client retry window, then clean up expired entries — an unbounded dedup table grows forever otherwise.
7. **Handle the in-flight case explicitly.** If a retry arrives while the original request is still executing (not yet completed), the correct response is usually to make the retry wait for the in-flight result (or return a `409`/`425` telling the client to retry later) — not to let a second execution start concurrently with the first.
8. **Layer idempotency into message consumers as defense in depth, not a replacement for broker guarantees.** At-least-once delivery (see `async-messaging-architecture`) means every consumer will occasionally see a redelivered message; dedup by message ID (or a business idempotency key carried in the message) at the consumer, in addition to making the handler's own effect idempotent where possible — relying on either alone is fragile.
9. **Make every saga step idempotent, including compensations.** An orchestrator will retry a failed step and may also invoke a compensating action that itself needs to be retried; if either isn't idempotent, saga retries create the exact duplicate-effect bugs the pattern was meant to avoid — see `saga-pattern-for-distributed-transactions`.
10. **Account for schema evolution across retries.** A request retried after a deploy that changed the request/response schema (see `api-versioning-strategy`) must still resolve against the idempotency record created under the old schema — decide whether the dedup store stores the raw original response verbatim (safest) or reconstructs it (risk of drift) so a retry after a deploy doesn't produce a subtly different response than the original call did.
11. **Document, per endpoint, whether it is idempotent and by what mechanism.** Client integrators need this stated explicitly (in API docs, not inferred) so they know which calls are safe to blindly retry and which require generating a fresh idempotency key per logical attempt.

## Example

Scenario: `POST /orders` in a checkout API accepts an `Idempotency-Key` header and creates an order plus an authorization hold on the customer's card.

Dedup store schema (Postgres table `idempotency_keys`):

```
key             text primary key      -- client-supplied UUID
request_hash    text not null         -- sha256 of normalized request body
status          text not null         -- 'in_progress' | 'completed' | 'failed'
response_body   jsonb                 -- stored response, returned verbatim on replay
response_status int
created_at      timestamptz not null
expires_at      timestamptz not null  -- created_at + 24h
```

Retry sequence that this design handles correctly:

```
t=0.0s   Client sends POST /orders, Idempotency-Key: 8f3a...
t=0.0s   Server: INSERT INTO idempotency_keys (key, status=in_progress) ON CONFLICT DO NOTHING
         -> insert succeeds, server proceeds to create order + auth hold
t=2.0s   Client's HTTP connection times out (no response received) — client does NOT know if it succeeded
t=2.0s   Client retries: POST /orders, same Idempotency-Key: 8f3a...
t=2.0s   Server: INSERT ... ON CONFLICT DO NOTHING -> conflict, key already exists, status=in_progress
         -> server makes retry wait briefly / returns 425 Too Early, does not re-create order
t=2.3s   Original request finishes: order created, response stored, status set to completed
t=2.4s   Client retries again: POST /orders, same key
         -> server finds status=completed, returns the STORED response verbatim, no new order or charge
```

Rejected alternatives:

| Alternative | Rejected because |
|---|---|
| Rely on the client not to retry | Network timeouts are indistinguishable from server failure from the client's view; a well-behaved client *must* retry ambiguous failures, so the server must be safe under that retry |
| Unique constraint on business fields alone (e.g., `UNIQUE(customer_id, cart_id)`) with no explicit key | Doesn't handle two concurrent identical requests both passing the check before either commits; also breaks legitimate re-orders of the same cart after a prior order completed and the cart was reused |
| Check-then-insert (SELECT for existing key, then INSERT if absent) | Races under concurrency — two near-simultaneous requests with the same key can both pass the SELECT before either INSERTs, both proceeding to charge the card |

## Common pitfalls

- **Treating idempotency as a synonym for "safe to retry blindly forever."** It guarantees the same effect/response, not that the operation is instantaneous or free of an in-flight window a fast-retrying client can still race into if not handled explicitly (step 7).
- **Check-then-act claim logic instead of an atomic claim.** This is the single most common bug: a lookup followed by a separate insert races under concurrency and defeats the entire mechanism.
- **Reusing an idempotency key for a logically different request and silently executing the new body.** Detect via request-hash comparison and reject with a conflict; don't guess which version the client "really" meant.
- **No TTL, or a TTL shorter than realistic client retry windows.** An unbounded dedup table grows forever; a too-short TTL lets a legitimate slow retry fall through and double-execute.
- **Assuming exactly-once delivery from a message broker makes consumer-side idempotency unnecessary.** No mainstream broker delivers exactly-once end-to-end in practice; consumer-side dedup and idempotent handlers are still required.
- **Idempotent happy path but non-idempotent compensating/rollback logic in a saga.** Retried compensations must be just as idempotent as the forward steps.
- **Storing a reconstructed response instead of the original verbatim response.** If the reconstruction logic drifts after a deploy, a retried request can get a response that doesn't match what actually happened the first time.

## Output

An idempotency design note per write endpoint or message handler (often embedded in the API's OpenAPI spec or a dedicated `docs/architecture/idempotency.md`) specifying: whether the operation is idempotent by construction or requires an explicit key, the dedup store schema and TTL, the atomic claim mechanism, the conflict behavior on key reuse with a differing body, and the documented client contract for generating and reusing keys — feeding directly into API documentation for integrators and into the saga/message-consumer retry design.
