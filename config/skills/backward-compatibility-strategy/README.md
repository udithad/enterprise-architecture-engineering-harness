# Backward Compatibility Strategy

A Claude Code skill for designing API contract and schema changes so old clients and consumers keep working during a gradual rollout, instead of coordinating a flag-day cutover that breaks whichever service redeploys last.

## Who it helps

- **Architects and tech leads** who need a mechanical classification (additive / additive-with-default / destructive) for whether a change is safe, instead of relying on a reviewer's judgment call.
- **Engineering managers** who need dual-write windows and schema migrations to have scheduled close-out dates instead of becoming permanent technical debt.
- **Developers** making a database or event-schema change touched by multiple independently-deployed services who need the concrete expand/migrate/verify/contract sequence.

## How it helps

- Turns every destructive change into a bounded expand/migrate/verify/contract sequence, with each intermediate state kept valid for both old and new code.
- Shows how to enforce compatibility mechanically — schema-registry `BACKWARD`/`FORWARD`/`FULL` modes and CI schema-diff gates — instead of trusting code review to catch a breaking Avro or protobuf change.
- Walks through a full worked example: splitting a free-text `address` column into structured fields across five independently-deployed services and a Kafka topic, with a table showing exactly what old and new code see at each phase.
- Cross-references `api-versioning-strategy` for changes that genuinely can't be made compatible, and distinguishes when to reach for a version bump versus when an additive/expand-contract sequence avoids needing one at all.

## How to use it

```bash
mkdir -p .claude/skills
cp -r backward-compatibility-strategy .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/backward-compatibility-strategy
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/backward-compatibility-strategy /path/to/your/project/.claude/skills/
```
