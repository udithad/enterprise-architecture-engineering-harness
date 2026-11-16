---
name: async-messaging-architecture
description: "Designs producer/consumer messaging systems (Kafka, RabbitMQ, SQS) with an explicit delivery guarantee, ordering scope, and backpressure mechanism, rather than defaulting to 'exactly-once' or 'FIFO' without checking whether the broker and consumer logic actually provide it. Use when introducing a queue or stream between services, when consumers are falling behind or duplicating work, or when a design review can't answer 'what happens if this message is delivered twice, out of order, or the consumer is slower than the producer.'"
---

# Async Messaging Architecture

Every messaging system makes exactly three promises you must pin down explicitly — delivery guarantee, ordering scope, and what happens when the consumer can't keep up — and every broker's marketing claim of "exactly-once" collapses under a precise definition to "at-least-once delivery plus idempotent processing." Design around what the broker actually gives you, not the label on the box.

## When to use this

- A synchronous call between two services is being replaced with a queue or event stream to decouple them or absorb load spikes.
- A design review can't answer what happens to in-flight messages during a consumer crash, deploy, or broker failover.
- Consumers are falling behind the producer (growing queue depth / consumer lag) and there's no documented backpressure policy.
- Two messages that must be processed in order are arriving out of order, or a "FIFO" queue is silently reordering across partitions/shards.
- A postmortem finds a message was processed twice (duplicate order, duplicate notification) or lost entirely during a rebalance or broker restart.
- A team is choosing between Kafka, RabbitMQ, SQS, or a cloud pub/sub service and needs the decision grounded in guarantees required, not vendor familiarity.

## Workflow

1. **Name the required delivery guarantee before picking a broker.** At-most-once (fire-and-forget, acceptable loss — e.g., non-critical telemetry) never retries and can lose messages on failure. At-least-once (the practical default for anything that matters) retries until acknowledged and can deliver duplicates. "Exactly-once" is only achievable end-to-end when the consumer's effect is naturally idempotent or made so explicitly (see `idempotency-design-for-apis`) — treat any broker's "exactly-once semantics" claim as scoped to its own internal offset bookkeeping, not to your downstream side effects (a DB write, an email send, a card charge).
2. **Pick the ordering scope explicitly: none, per-key, or global.** Global total ordering across a topic caps throughput at a single-partition/single-consumer rate and rarely justifies its cost. Per-key ordering (Kafka: same key → same partition; RabbitMQ: single consumer per queue with prefetch=1) is the usual right answer — order is only guaranteed within a partition/shard, so choose the partition key to match the actual invariant you need ordered (e.g., all events for one `order_id`, not one global sequence).
3. **Decide the acknowledgment point relative to processing.** Ack-before-process (auto-commit before handling) risks losing the message if the consumer crashes mid-processing — never do this for anything that isn't disposable. Ack-after-process (commit offset / delete from queue only after the handler completes and persists its effect) is the standard safe default, at the cost of possible reprocessing (hence step 1's idempotency requirement) if the ack itself fails after processing succeeded.
4. **Design backpressure as a first-class mechanism, not an afterthought.** Options, not mutually exclusive: bounded consumer prefetch/window (RabbitMQ `prefetch_count`, Kafka `max.poll.records`) to cap in-flight work per consumer; autoscaling consumers on queue-depth or consumer-lag metrics; load shedding at the producer (reject or redirect to a lower-priority path) once queue depth crosses a threshold; and a dead-letter queue for messages that repeatedly fail rather than blocking the whole partition behind a poison message.
5. **Choose the broker based on the guarantees and access pattern needed, not familiarity.** Kafka: durable replayable log, high throughput, per-partition ordering, consumer groups re-read history — fits event sourcing, stream processing, and multi-consumer fan-out where replay matters. RabbitMQ: smart broker with routing (exchanges), per-message TTL/priority, simpler point-to-point and work-queue semantics — fits task queues and complex routing topologies. SQS/cloud queues: managed, at-least-once, optional FIFO with per-message-group ordering — fits simpler decoupling without operating broker infrastructure.
6. **Size the partition/shard count for your target parallelism up front.** Kafka partition count is a hard ceiling on consumer parallelism within a consumer group and is expensive to change later (repartitioning breaks existing key-to-partition assignment, silently breaking per-key ordering guarantees mid-flight) — provision for projected peak consumer count, not current.
7. **Design the poison-message path.** After N delivery attempts (tracked via a retry counter in the message or broker-native retry count), route to a dead-letter queue/topic instead of retrying forever or dropping silently. Alert on DLQ depth; a silently growing DLQ is a slow-motion data-loss incident.
8. **Make consumer processing idempotent regardless of the broker's guarantee.** At-least-once delivery is the practical ceiling for almost every broker under failure and rebalance conditions — build consumer-side dedup (by message ID or business key) as defense in depth rather than trusting broker configuration alone; see `idempotency-design-for-apis` for the mechanism.
9. **Version message schemas with the same discipline as an API contract.** A consumer deployed behind the producer must tolerate new optional fields and must not break on an unrecognized enum value; use a schema registry (Confluent Schema Registry, AWS Glue Schema Registry) with compatibility mode enforcement (`BACKWARD`/`FORWARD`) rather than trusting convention — see `backward-compatibility-strategy`.
10. **Plan for consumer-group rebalancing and duplicate/gap windows explicitly.** A rebalance (consumer joins/leaves/crashes) reassigns partitions and can cause a brief window of duplicate processing (message reprocessed by new owner before old owner's in-flight work is accounted for) — the idempotency work from step 8 is what makes this safe, not a broker configuration flag.
11. **Distinguish this from request/response resilience patterns that look similar but solve different problems.** A circuit breaker (`circuit-breaker-pattern-design`) protects a caller from a failing synchronous dependency; a saga (`saga-pattern-for-distributed-transactions`) coordinates a multi-step business transaction, often over messaging — async messaging architecture is the substrate those patterns can run on top of, not a replacement for either.
12. **Document consumer lag and end-to-end latency SLOs, and alert on both independently.** Queue depth alone hides whether a backlog is because the producer spiked (fine, if consumers catch up) or a consumer regressed (not fine) — track consumer lag (messages behind head) and processing latency per message as separate signals feeding into `observability-architecture-design`.

## Example

Scenario: RideNow, a ride-hailing platform, publishes `trip.completed` events from the trip service to Kafka, consumed by a billing service (charges the rider) and a driver-payout service (credits the driver). Peak load: 3,000 events/sec at rush hour, sustained 400/sec otherwise.

Design:

```
Topic: trip.completed, 24 partitions, key = trip_id
Producer: acks=all, idempotent producer enabled (producer-side dedup on retry)
Consumer groups: billing-service (24 consumers max, 1:1 with partitions), driver-payout-service (separate group, independent lag)
Ack point: after DB write commits (offset committed post-processing, not pre-)
Retry: 3 attempts with exponential backoff, then -> trip.completed.dlq
DLQ alert: PagerDuty if dlq depth > 50 or growth rate > 10/min
```

Delivery-guarantee reasoning: `trip_id` as partition key guarantees all events for one trip are processed in order by one consumer at a time within a group — required because a `trip.completed` event must not be processed before a hypothetical earlier `trip.started` correction. Kafka's producer idempotence + at-least-once consumer acknowledgment means billing must dedup by `trip_id` before charging (idempotency key = `trip_id`, per `idempotency-design-for-apis`) — the system is at-least-once end-to-end, with idempotent billing making it effectively exactly-once in outcome.

Rejected alternatives:

| Alternative | Rejected because |
|---|---|
| Single partition for global ordering | Caps throughput at one consumer's processing rate (~200/sec measured), far below the 3,000/sec rush-hour peak |
| At-most-once (fire-and-forget, no retry) | A dropped `trip.completed` event means a driver never gets paid — unacceptable data loss for a financial event |
| Trust Kafka's "exactly-once semantics" (transactional producer/consumer) alone, no consumer-side idempotency | EOS only guarantees exactly-once *within Kafka's own offset/transaction bookkeeping*; it does not make the billing service's external DB write + payment-gateway call exactly-once — a crash between Kafka transaction commit and the external charge call would still double-charge without consumer-side dedup |

## Common pitfalls

- **Assuming a broker's "exactly-once" feature makes downstream side effects exactly-once.** It scopes only to the broker's internal state; any external effect (DB write, API call, email) still needs idempotent handling.
- **Choosing global ordering by default "to be safe."** Collapses throughput to a single consumer's rate; scope ordering to the actual key that needs it (per-entity, not global).
- **Acking before processing completes.** Loses messages on consumer crash; silently trades durability for a marginal latency win.
- **No backpressure mechanism, relying on the broker's buffer to absorb any producer/consumer speed mismatch.** Buffers are finite; an unbounded producer against a slow consumer eventually hits broker disk limits or unbounded memory growth.
- **No dead-letter path, so a single malformed or poison message blocks an entire partition behind endless retries.** Blocks every other message keyed to that partition, not just the bad one.
- **Growing Kafka partition count "for more parallelism" without accounting for the resulting change to key-to-partition assignment.** Breaks per-key ordering guarantees for in-flight and replayed messages during the transition.
- **Treating consumer lag and queue depth as the same signal.** A lag spike from a producer burst is often self-healing; a lag spike from a stuck or crashed consumer needs paging — alert on them differently.

## Output

A messaging design doc (commonly `docs/architecture/messaging/<topic-name>.md`) specifying: the delivery guarantee and its scope (broker-internal vs. end-to-end with idempotency), the partition/shard key and ordering scope, the ack-point and retry/backoff policy, the backpressure mechanism (bounded prefetch, autoscaling trigger, load-shedding threshold), and the dead-letter and alerting policy — feeding into the consumer's idempotency design (`idempotency-design-for-apis`), the message schema's compatibility rules (`backward-compatibility-strategy`), and the observability dashboards tracking consumer lag and DLQ depth.
