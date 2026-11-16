# Event-Driven Architecture Design

A Claude Code skill for designing systems where services communicate via published events instead of direct synchronous calls — covering schema design, choreography vs. orchestration, delivery guarantees, and broker topology.

## Who it helps

- **Architects and tech leads** deciding whether a synchronous call chain should become event-driven, and choosing between choreography and orchestration for a given flow.
- **Engineering managers** who need to explain why a redesign introduces eventual consistency and what guarantees consumers can actually rely on.
- **Developers** building event producers or consumers who need a concrete checklist for schema versioning, idempotency, and dead-letter handling before shipping.

## How it helps

- Gives a decision framework for event notification vs. event-carried state transfer, and for choreography vs. orchestration, instead of defaulting to whichever pattern was used last time.
- Covers delivery-guarantee trade-offs (at-least-once + idempotency vs. the cost of true exactly-once) with concrete implications for consumer design.
- Walks through a full worked example: a ride-share trip-completion flow moving from a synchronous call chain to a Kafka-based choreography, with the orchestration alternative explicitly considered and rejected.
- Calls out failure modes specific to event systems — thin-event callback chains, schema drift, poison messages, choreography sprawl — that don't show up in synchronous-API design.

## How to use it

```bash
mkdir -p .claude/skills
cp -r event-driven-architecture-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/event-driven-architecture-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/event-driven-architecture-design /path/to/your/project/.claude/skills/
```
