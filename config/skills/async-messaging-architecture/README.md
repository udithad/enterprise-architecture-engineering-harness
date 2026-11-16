# Async Messaging Architecture

A Claude Code skill for designing producer/consumer messaging systems with an explicit delivery guarantee, ordering scope, and backpressure mechanism, instead of assuming a broker's "exactly-once" or "FIFO" label means what it sounds like.

## Who it helps

- **Architects and tech leads** choosing between Kafka, RabbitMQ, and cloud queues based on the actual ordering and delivery guarantees a system needs, not broker familiarity.
- **Engineering managers** who need consumer lag, DLQ growth, and backpressure treated as first-class operational signals instead of discovered during an incident.
- **Developers** building producers and consumers who need a concrete pattern for ack timing, partition-key selection, and poison-message handling.

## How it helps

- Breaks down why "exactly-once" is a broker-internal claim, not an end-to-end one, and shows exactly where consumer-side idempotency has to close the gap.
- Gives concrete guidance on ordering scope (none/per-key/global) tied to partition-key choice, instead of defaulting to global ordering and capping throughput.
- Walks through a full worked example: a ride-hailing platform's `trip.completed` topic, including partition count, ack point, retry/DLQ policy, and the exact reasoning for why it's at-least-once end-to-end rather than truly exactly-once.
- Cross-references `idempotency-design-for-apis` for consumer-side dedup, `backward-compatibility-strategy` for schema evolution, and `circuit-breaker-pattern-design`/`saga-pattern-for-distributed-transactions` for how those patterns sit on top of a messaging substrate rather than duplicating it.

## How to use it

```bash
mkdir -p .claude/skills
cp -r async-messaging-architecture .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/async-messaging-architecture
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/async-messaging-architecture /path/to/your/project/.claude/skills/
```
