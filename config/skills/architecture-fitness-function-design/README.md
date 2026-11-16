# Architecture Fitness Function Design

A Claude Code skill for turning architectural rules — layering, dependency direction, latency budgets, resiliency requirements — into automated tests that run in CI, so architecture erosion is caught at PR time instead of at the next manual review.

## Who it helps

- **Architects and tech leads** who have documented rules (in ADRs or diagrams) that keep getting silently violated because nothing actually enforces them.
- **Engineering managers** who want objective, trend-able evidence of architecture health instead of relying on anecdote at review time.
- **Developers** who want fast, specific CI feedback ("domain layer imported Spring") instead of finding out about a violation months later in a painful review.

## How it helps

- Classifies rules by type (structural, latency, resiliency, security, process) and matches each to the right off-the-shelf tool (ArchUnit, import-linter, depcruise, load-test assertions) instead of bespoke scripts.
- Gives a rollout path — warning severity plus a dated allowlist, then blocking — so turning a rule on doesn't halt the whole team over pre-existing violations.
- Walks through a full worked example: a hexagonal-architecture Java codebase adding ArchUnit rules to stop domain-layer framework leakage, plus a temporary rule protecting a strangler-fig migration boundary.
- Flags the trap of an allowlist that never shrinks, and of fitness functions that outlive the migration they were built to protect.

## How to use it

```bash
mkdir -p .claude/skills
cp -r architecture-fitness-function-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/architecture-fitness-function-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/architecture-fitness-function-design /path/to/your/project/.claude/skills/
```
