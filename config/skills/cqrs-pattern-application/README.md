# CQRS Pattern Application

A Claude Code skill for splitting a service's write model from its read model when read and write workloads have genuinely different shapes or scaling needs — with a strong bias toward confirming the symptom before adopting the pattern.

## Who it helps

- **Architects and tech leads** deciding whether a service's read/write contention or query complexity actually justifies CQRS versus simpler fixes like an index or a read replica.
- **Engineering managers** who need to weigh the added operational complexity (a second model, a projector, eventual consistency) against the performance problem it solves.
- **Developers** implementing the read-model projection layer who need a concrete pattern for outbox-based projection, staleness handling, and projector replay.

## How it helps

- Leads with a "confirm the symptom first" step, since CQRS is one of the most over-applied patterns in system design — pushes back on adopting it for simple CRUD.
- Lays out the projection mechanism choices (synchronous dual-write vs. transactional outbox vs. event sourcing) with the specific failure mode that rules out the naive option.
- Walks through a full worked example: a hotel-booking service splitting one contended Postgres schema into a write model plus three purpose-built read models (Postgres, DynamoDB, Elasticsearch), including a read pattern deliberately left un-split.
- Distinguishes CQRS from event sourcing explicitly, since the two are often conflated and forcing both on a team that only needed one adds needless complexity.

## How to use it

```bash
mkdir -p .claude/skills
cp -r cqrs-pattern-application .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/cqrs-pattern-application
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/cqrs-pattern-application /path/to/your/project/.claude/skills/
```
