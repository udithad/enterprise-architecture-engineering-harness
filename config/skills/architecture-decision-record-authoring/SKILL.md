---
name: architecture-decision-record-authoring
description: "Guides writing a clear, durable Architecture Decision Record (ADR) that captures context, options considered, and rationale. Use when a team is about to make, or has just made, a consequential technical decision that future engineers will need to understand without re-litigating it."
---

# Architecture Decision Record Authoring

Capture a significant technical decision — what was decided, why, and what alternatives were rejected — in a short, durable document so future engineers don't have to reverse-engineer the reasoning from Slack history or git blame.

## When to use this

- A team is choosing between two or more viable technical approaches (e.g., REST vs gRPC, Postgres vs DynamoDB, monolith vs services) and the choice is hard to reverse later.
- Someone asks "why did we build it this way?" and nobody has a confident answer.
- A design review, RFC, or architecture review board meeting concludes with a decision.
- Onboarding a new engineer who keeps re-proposing something that was already tried and rejected.
- Before a large refactor or migration, to record the decision to proceed and the constraints it must satisfy.
- After an incident postmortem recommends a structural change to the system.

Do not use this for reversible, low-stakes choices (variable naming, a single function's internal structure) — ADRs are for decisions with real switching cost.

## Workflow

1. **Confirm the decision is ADR-worthy.** Ask: would reversing this cost more than a day of work, or affect more than one team? If no, skip the ADR.
2. **Assign the next sequential number.** Look in `docs/adr/` for the highest existing `NNNN-*.md` and increment. Numbers are never reused, even for rejected/superseded ADRs.
3. **Write the title as a decision, not a topic.** Good: "Use PostgreSQL row-level security for tenant isolation." Bad: "Database choice."
4. **Fill in Context.** State the problem being solved, the forces in tension (e.g., cost vs latency, consistency vs availability), and any constraints already fixed by prior decisions (link them).
5. **List the options actually considered**, each with a one-paragraph summary of its trade-offs. Include the option you rejected even if it seems obviously wrong in hindsight — that's the point of the record.
6. **State the Decision** in one or two sentences, unambiguous, in the imperative ("We will use X").
7. **List Consequences** — both positive and negative. Every real decision has downsides; naming them is what makes the ADR trustworthy rather than a sales pitch.
8. **Set Status**: `Proposed` while under review, `Accepted` once agreed, `Superseded by NNNN` if a later ADR replaces it, `Deprecated` if abandoned without replacement.
9. **Get it reviewed** by whoever owns the affected system(s) before merging — an ADR merged without review is just a diary entry.
10. **Link it** from the README or architecture index of the affected component, and cross-link to related ADRs (both directions).

## Example

Scenario: A payments team is deciding how to handle idempotency for a new `POST /charges` endpoint that's occasionally double-invoked by a flaky upstream retry policy.

```markdown
# 0017-idempotency-keys-for-charge-creation.md

## Status
Accepted

## Context
The `POST /charges` endpoint is called by the checkout service, which retries
on any 5xx or timeout. Under load, ~0.3% of charge requests are retried at
least once. Without deduplication, retries can create duplicate charges,
which is a direct financial and trust problem. We need a way to make retried
requests safe without requiring the caller to poll for a prior result first.

## Options Considered

**1. Client-supplied idempotency key (chosen)**
Caller generates a UUID per logical charge attempt and sends it as
`Idempotency-Key` header. Server stores (key -> response) for 24h and returns
the cached response on replay. Industry-standard pattern (Stripe, Adyen).
Requires client discipline to reuse the same key on retry.

**2. Server-side request fingerprinting**
Hash (customer_id, amount, order_id, timestamp bucket) to detect duplicates
without a client-supplied key. Rejected: timestamp bucketing is fragile —
legitimate same-amount repeat purchases within the window would be
incorrectly deduplicated.

**3. Two-phase reserve-then-capture**
Caller reserves a charge slot, then confirms. Rejected: doubles the number
of round trips and doesn't remove the double-invocation risk on the confirm
call itself.

## Decision
We will implement client-supplied idempotency keys, stored in a
`idempotency_keys` table (key, request_hash, response_body, status,
created_at) with a 24-hour TTL enforced by a nightly cleanup job. If a key
is replayed with a different request body, return 422 Unprocessable Entity
rather than silently returning the cached response.

## Consequences
- Positive: Retries become safe by construction; matches the pattern
  checkout-service clients already expect from other payment providers.
- Positive: No distributed lock needed; a unique constraint on the key
  column handles concurrent duplicate inserts.
- Negative: Clients that don't reuse keys on retry get no protection — this
  must be documented prominently in the API contract and checked in
  integration tests.
- Negative: Adds a new table and a cleanup job that must be operated and
  monitored (alert if cleanup job stops running, since the table grows
  unbounded otherwise).
```

## Common pitfalls

- **Writing the ADR after the fact, from memory.** Context and rejected alternatives are the first things forgotten. Draft it during or immediately after the discussion.
- **Only recording the winning option.** An ADR that lists no alternatives can't explain why the choice was made, only what it was.
- **Vague consequences ("this should scale well").** Prefer concrete, falsifiable statements ("this adds one extra DB round trip per write").
- **Treating ADRs as permanent contracts.** They record a decision at a point in time. If circumstances change, write a new ADR that supersedes the old one rather than editing history.
- **No review before merge.** An unreviewed ADR often means the trade-offs were never actually checked by someone with the context to catch a flaw.
- **Numbering collisions from parallel branches.** Reserve the number in the same PR that adds the file, and rebase-check before merging if two ADRs land close together.
- **Burying ADRs where no one finds them.** An ADR nobody links to is as good as undocumented — always link from the component's README or architecture index.

## Output

A single Markdown file at `docs/adr/NNNN-short-decision-title.md` (four-digit, zero-padded, sequential number) with the sections: `Status`, `Context`, `Options Considered`, `Decision`, `Consequences`. The file is added to version control alongside the code it affects, referenced from the relevant component's README, and cross-linked to any ADRs it supersedes or is superseded by.
