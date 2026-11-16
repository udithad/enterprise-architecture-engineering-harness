---
name: saga-pattern-for-distributed-transactions
description: "Designs a saga — a sequence of local transactions with compensating actions — to maintain data consistency across multiple services without a distributed ACID transaction. Use when a business operation spans multiple services, each owning its own database, and needs an explicit strategy for partial-failure rollback."
---

# Saga Pattern for Distributed Transactions

Coordinate a business operation that spans multiple services — each with its own database (see `database-per-service-design`) — as a sequence of local transactions, each paired with a compensating action that undoes its effect if a later step fails, since a distributed ACID transaction (two-phase commit) across service boundaries doesn't scale and couples services tightly.

## When to use this

- A business operation (e.g., "place an order") touches multiple services that each own their own data, and a failure partway through needs a defined, tested way to leave the system in a consistent state rather than a half-committed mess.
- A monolith-to-microservices migration (`monolith-to-microservices-migration-plan`) is breaking apart a transaction that used to be a single database transaction, and the team needs an explicit replacement strategy.
- The current approach to multi-service consistency is "hope nothing fails between step 2 and step 3," with no compensating logic and manual cleanup after incidents.
- A two-phase-commit or distributed-lock approach is being considered and its availability/latency cost (every participant must be up and locked for the duration) needs a lighter-weight alternative.

## Workflow

1. **Decompose the business operation into a sequence of local transactions**, each scoped to exactly one service/database, each independently committable. This is the saga's steps.
2. **Define a compensating transaction for every step that has a side effect worth undoing** — not every step needs one (a read has nothing to compensate), but every state-changing step needs an explicit, idempotent "undo."
3. **Choose choreography or orchestration deliberately.** Choreography (each service listens for events and reacts, no central coordinator) scales well and has no single point of control, but the overall flow becomes implicit, spread across services, and harder to observe/debug as steps grow. Orchestration (a central saga orchestrator issues commands and tracks state) makes the flow explicit and easy to monitor, but introduces a coordinating component that must itself be reliable.
4. **Design every step and every compensation to be idempotent**, since retries (from at-least-once messaging or orchestrator crash-recovery) will re-invoke them — a non-idempotent compensation applied twice (e.g., refunding twice) is a correctness bug of its own.
5. **Accept and design around eventual, not immediate, consistency** — between step 1 committing and step 3 either completing or triggering compensation, the system is in a genuinely intermediate state. Decide what's visible to users/other systems during that window and whether it needs to be hidden (a "pending" status) or is acceptable as-is.
6. **Handle the "compensation itself can fail" case explicitly** — a naive saga assumes compensations always succeed; a robust one has a retry policy for compensations and a defined escalation path (alert a human, quarantine the record) when compensation repeatedly fails.
7. **Persist saga state durably at every step** (in the orchestrator's own store, or derivable from the event log in choreography) so a crashed orchestrator or a restarted service can resume or safely re-drive the saga from where it left off, rather than losing track of in-flight sagas.
8. **Instrument the saga for observability** — a saga ID that ties together every step and compensation's logs (feeding into `observability-architecture-design`), and a way to query "what sagas are currently stuck/pending" for operational visibility.
9. **Test the failure paths as rigorously as the happy path** — every step's compensation should be exercised in tests, including compensation-of-a-compensation scenarios and the case where a step's forward action succeeded but its acknowledgment was lost (forcing a retry that must not double-apply).
10. **Document the saga's steps and compensations as part of the architecture** (a sequence diagram, see `sequence-diagram-authoring`), since an undocumented saga is nearly impossible for a new engineer to safely modify.

## Example

Scenario: `TripBooker` processes a trip booking that spans three services: `Flights`, `Hotels`, and `Payments`, each with its own database. Booking a trip means reserving a flight seat, reserving a hotel room, then charging the customer — and any failure after a reservation must release it.

Orchestrated saga design:

1. **Step 1 — Reserve flight**: `Flights` service reserves a seat, returns `reservation_id`. Compensation: `releaseFlightReservation(reservation_id)`, idempotent (releasing an already-released reservation is a no-op, not an error).
2. **Step 2 — Reserve hotel**: `Hotels` service reserves a room. Compensation: `releaseHotelReservation(reservation_id)`, same idempotency guarantee.
3. **Step 3 — Charge payment**: `Payments` service charges the customer's card, using an idempotency key equal to the saga ID (reusing `idempotency-design-for-apis`) so a retried charge command never double-charges. No compensation needed if this step is last and succeeds; if it fails, compensations for steps 1 and 2 run in reverse order.

Orchestrator behavior:
- Saga state (`PENDING_FLIGHT`, `PENDING_HOTEL`, `PENDING_PAYMENT`, `COMPLETED`, `COMPENSATING`, `FAILED`) persisted after every transition in the orchestrator's own durable store, keyed by `saga_id`.
- If payment fails (e.g., card declined), orchestrator transitions to `COMPENSATING`, issues `releaseHotelReservation` then `releaseFlightReservation`, in reverse order of the forward steps.
- If `releaseHotelReservation` itself fails (Hotels service temporarily down), the orchestrator retries with exponential backoff up to 5 attempts; if still failing, the saga is marked `FAILED_COMPENSATION` and an alert pages the on-call engineer with the saga ID — it is not silently dropped.
- The user-facing trip status shows "Confirming your booking…" during `PENDING_*` states rather than a misleadingly final "Confirmed," reflecting the genuine eventual-consistency window.

Result: a payment decline correctly and automatically releases both reservations within seconds in the normal case, and the rare case where compensation itself fails is surfaced operationally rather than silently leaving an orphaned reservation.

## Common pitfalls

- **Treating a saga as a distributed transaction with rollback guarantees** — it is not atomic; there is a real window where intermediate state is visible, and the design must account for that rather than assume it away.
- **Non-idempotent compensations**, causing double-refunds or double-releases when a compensation is retried after a crash or timeout.
- **No plan for compensation failure** — assuming the "undo" step always works is the most common way sagas silently leave orphaned state in production.
- **Choreography that grows into an untraceable web of event listeners** as more steps are added, with no single place to see the whole flow — worth reconsidering orchestration once a saga exceeds ~4-5 steps.
- **Losing saga state on orchestrator crash** because state wasn't persisted at each transition, leaving in-flight sagas unrecoverable.
- **Not testing compensation paths**, so the first real test of "what happens when step 3 fails" is a production incident.
- **Using a saga where a single-service transaction would do** — if the operation doesn't actually need to span multiple independently-owned databases, a saga adds complexity without benefit.

## Output

A saga design doc (or section of the service's architecture doc) listing: each step and its compensating action with idempotency guarantees stated, the choreography-vs-orchestration choice and rationale, the persisted saga state machine, the compensation-failure escalation policy, and a sequence diagram showing both the happy path and at least one compensation path.
