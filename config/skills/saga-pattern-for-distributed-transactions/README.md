# Saga Pattern for Distributed Transactions

A Claude Code skill for designing sagas — sequences of local transactions paired with compensating actions — to maintain data consistency across multiple independently-owned service databases without a distributed ACID transaction.

## Who it helps

- **Architects and tech leads** breaking a former single-database transaction into a multi-service flow during a microservices migration, who need an explicit, tested rollback strategy.
- **Engineering managers** who need "what happens when step 3 of 5 fails" answered concretely before launch, not discovered during the first production incident.
- **Developers** implementing saga steps who need a concrete pattern for idempotent compensation, persisted saga state, and handling the case where compensation itself fails.

## How it helps

- Frames the core decision — choreography vs. orchestration — around observability and flow complexity, not just theoretical elegance.
- Forces idempotency on both forward steps and compensations, closing the most common source of saga correctness bugs (double-charges, double-releases on retry).
- Covers the failure mode most saga designs skip: what happens when the compensation itself fails, with an explicit escalation path instead of silent orphaned state.
- Walks through a full worked example: a trip-booking flow across Flights, Hotels, and Payments services, including a persisted state machine and a compensation-failure alert path.

## How to use it

```bash
mkdir -p .claude/skills
cp -r saga-pattern-for-distributed-transactions .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/saga-pattern-for-distributed-transactions
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/saga-pattern-for-distributed-transactions /path/to/your/project/.claude/skills/
```
