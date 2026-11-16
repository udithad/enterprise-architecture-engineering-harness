---
name: event-driven-architecture-design
description: "Designs systems where services communicate via published events rather than direct calls, covering event schema design, choreography vs. orchestration, delivery guarantees, and broker topology. Use when decoupling services temporally, fanning out one business fact to many consumers, or replacing brittle synchronous call chains with asynchronous event flows."
---

# Event-Driven Architecture Design

Structure a system around facts that happened (`OrderPlaced`, `PaymentCaptured`) published to a broker and consumed independently, so producers don't need to know who's listening or block on their response. This buys temporal decoupling and easy fan-out at the cost of harder-to-trace flows and eventual consistency.

## When to use this

- One service's write needs to trigger side effects in three or more other services (notifications, analytics, search indexing) and synchronous fan-out calls are making the producer's latency and failure rate depend on the slowest/flakiest consumer.
- Consumers need to be added over time without the producer changing code or even knowing they exist (e.g., a new fraud-detection service that starts consuming `OrderPlaced` six months after launch).
- The business process is naturally a sequence of state changes over time (order lifecycle, shipment tracking) rather than a single request/response.
- A synchronous call chain (`A calls B calls C calls D`) has become fragile — one slow downstream service degrades everyone upstream of it.
- You need an audit trail or the ability to replay history (rebuild a read model, debug "what happened to order 4471") — an event log gives you this for free; a call-and-forget API doesn't.
- Someone is proposing "just add a webhook for every new consumer" and the producer's list of direct callees is growing unmanageably.

## Workflow

1. **Distinguish event notification from event-carried state transfer.** A thin notification (`OrderPlaced { orderId }`) forces consumers to call back for details (reintroduces coupling); a fat event (`OrderPlaced { orderId, items, total, customerId }`) lets consumers work standalone but risks leaking internal data model and growing unbounded. Pick per event type based on how autonomous consumers need to be — don't default to one style everywhere.
2. **Decide choreography vs. orchestration up front.** Choreography (each service reacts to events and emits its own, no central coordinator) scales well for simple fan-out but becomes untraceable past 4-5 hops — nobody can answer "what triggers a refund?" without grepping every service. Orchestration (a central process manager drives the steps) is easier to observe and reason about but reintroduces a coordinating single point of ownership. For multi-step business transactions with compensation logic, prefer orchestration (see `saga-pattern-for-distributed-transactions`); for simple "notify N interested parties" fan-out, choreography is fine.
3. **Design events as an explicit, versioned contract**, not a serialization of an internal object. Define a schema (Avro/Protobuf/JSON Schema) per event type, register it in a schema registry, and treat it with the same rigor as a public API (see `api-versioning-strategy`, `backward-compatibility-strategy`). Additive-only changes (new optional fields) are safe; renaming or removing fields breaks every consumer silently.
4. **Choose delivery guarantees deliberately.** At-least-once (the common default) means consumers must be idempotent (see `idempotency-design-for-apis`) because redelivery will happen. Exactly-once is expensive and usually a broker-level illusion built on at-least-once + dedup; don't promise it to the business without understanding the mechanism underneath.
5. **Pick a broker topology to match ordering and retention needs.** A log-based broker (Kafka, Pulsar) gives ordered partitions and long retention/replay — good for event sourcing or audit needs. A queue-based broker (RabbitMQ, SQS) gives simpler point-to-point/competing-consumer semantics but weaker replay. Don't pick Kafka by default if nobody needs replay or ordering — it's heavier to operate.
6. **Define partitioning/ordering keys explicitly.** Events for the same aggregate (e.g., same `orderId`) must land in the same partition to preserve per-entity ordering; cross-aggregate ordering is generally not guaranteed and should not be assumed by any consumer.
7. **Plan for consumer failure and poison messages.** Every consumer needs a dead-letter queue, a retry policy with backoff, and an alert when the DLQ grows — an event silently dropped after 3 failed retries is a production incident nobody notices until a customer complains.
8. **Design for schema evolution and consumer independence.** Each consumer should tolerate unknown fields (avoid strict schema validation that rejects new optional fields) so producers can evolve without a synchronized deploy across every consumer.
9. **Decide how synchronous "read after write" needs are met.** If a client publishes `OrderPlaced` and then immediately queries "is my order placed?", the read path may hit a stale read model. Either expose a synchronous confirmation API alongside the event, or make the client tolerant of eventual consistency (see `data-consistency-model-selection`).
10. **Instrument for traceability from day one.** Propagate a correlation/causation ID through every event so a request that fans out across 6 services can be reconstructed in one trace (see `observability-architecture-design`) — retrofitting this after an incident is far more painful.
11. **Decide what "done" means for compliance/audit needs.** If events are the system of record (event sourcing), define retention, replay, and snapshotting strategy explicitly; if events are just notifications and a database is still the system of record, say so clearly so nobody assumes the event log is durable truth.

## Example

Scenario: `RideShare Co.` currently has `Trip Service` synchronously call `Billing Service`, `Driver Payout Service`, and `Rider Notification Service` when a trip ends. A slowdown in Notification Service (a 3rd-party SMS provider outage) has twice delayed billing because the calls are chained sequentially with retries.

Redesign: `Trip Service` publishes a `TripCompleted` event to a Kafka topic `trip-events`, partitioned by `tripId`, containing fare breakdown, driver ID, rider ID, and distance/duration (event-carried state transfer — consumers shouldn't need to call back into Trip Service for basic trip facts).

```
Trip Service --publishes--> trip-events topic (partitioned by tripId)
                                  |-- Billing Service (consumer group: billing)     -> charges rider
                                  |-- Payout Service (consumer group: payout)       -> credits driver
                                  |-- Notification Service (consumer group: notify) -> sends SMS/push
                                  |-- Analytics Service (consumer group: analytics) -> feeds dashboards
```

Choice: choreography, not orchestration — these four consumers act independently with no dependency between each other's outcomes, and there's no compensation logic needed if one fails (each retries independently with its own DLQ). Orchestration was considered and rejected because there's no cross-service rollback requirement here (contrast with a saga where a payout failure must trigger a billing reversal — that would justify orchestration).

Delivery: at-least-once with Billing and Payout made idempotent via a `tripId` uniqueness constraint on their local ledger tables — a redelivered `TripCompleted` is a no-op, not a double-charge.

Result: Notification Service's SMS outage no longer blocks billing or payout — it retries against its own DLQ, and Billing/Payout are unaffected since they consume independently.

## Common pitfalls

- **Thin events that force callback chains.** An event with only an ID forces every consumer to call back to the producer for details, recreating synchronous coupling with extra steps and extra latency.
- **No schema versioning discipline.** A producer renames a field or changes a type and every consumer silently breaks (or worse, silently misinterprets data) because nobody was validating against a registered schema.
- **Assuming exactly-once without doing the idempotency work.** At-least-once + non-idempotent consumer = duplicate charges, duplicate emails, duplicate payouts during any redelivery (broker restart, rebalance, timeout).
- **Choreography sprawl.** Ten services choreographing off each other's events with no shared documentation becomes impossible to reason about — nobody can answer "what happens when X publishes Y" without live-tailing every topic.
- **Using events as a distributed transaction without a saga design.** Firing events and hoping downstream failures "sort themselves out" without a compensation strategy leaves the system in silently inconsistent states (see `saga-pattern-for-distributed-transactions`).
- **No dead-letter handling.** A poison message that can't be processed retries forever (blocking the partition) or is silently dropped — both are invisible until someone notices missing data.
- **Treating the event log as a message queue with unlimited retention for free.** Retention has real storage cost and compaction trade-offs; decide retention window deliberately, not by broker default.

## Output

An event-driven design document (e.g., `docs/architecture/event-design.md`) listing: each event type with its schema and versioning policy, producer/consumer map, chosen delivery guarantee and idempotency strategy per consumer, broker/topology choice with partitioning keys, and an explicit choreography-vs-orchestration call per business flow with rejected alternatives — feeding into ADRs and into the schema registry as the enforced contract.
