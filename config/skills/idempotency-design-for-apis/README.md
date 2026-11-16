# Idempotency Design for APIs

A Claude Code skill for making retries safe — guaranteeing a repeated request produces the same result as the original — instead of hoping clients never retry or assuming "idempotent" means "safe" and "atomic" for free.

## Who it helps

- **Architects and tech leads** deciding which write endpoints are idempotent by construction (PUT, conditional writes, upserts) versus which need an explicit idempotency-key mechanism.
- **Engineering managers** responding to an incident where a customer was double-charged or a duplicate order was created because of a retried request.
- **Developers** implementing a payment endpoint, message consumer, or saga step that must not double-execute under at-least-once delivery or client-side retries.

## How it helps

- Draws the line between "idempotent," "safe," and "atomic" precisely, and gives a concrete atomic-claim mechanism (`INSERT ... ON CONFLICT DO NOTHING`) instead of the check-then-act race that quietly breaks most naive implementations.
- Walks through a full worked example: a `POST /orders` checkout endpoint with a `idempotency_keys` dedup table and a millisecond-by-millisecond retry timeline showing exactly how a timed-out request, an in-flight retry, and a completed-request retry are each handled.
- Compares rejected alternatives (relying on clients not to retry, a unique constraint on business fields alone, check-then-insert) with the specific race condition each one leaves open.
- Cross-references `saga-pattern-for-distributed-transactions` and `async-messaging-architecture` so idempotency is treated as defense-in-depth alongside broker guarantees and compensating actions, not a replacement for either.

## How to use it

```bash
mkdir -p .claude/skills
cp -r idempotency-design-for-apis .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/idempotency-design-for-apis
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/idempotency-design-for-apis /path/to/your/project/.claude/skills/
```
