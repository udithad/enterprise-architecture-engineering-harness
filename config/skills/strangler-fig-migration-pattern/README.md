# Strangler Fig Migration Pattern

A Claude Code skill for incrementally replacing a legacy system behind a routing façade — moving traffic to a new implementation feature by feature until legacy can be safely retired — instead of a risky big-bang rewrite.

## Who it helps

- **Architects and tech leads** planning a multi-quarter legacy replacement who need a concrete, risk-ordered sequence of migration pieces instead of an all-at-once cutover plan.
- **Engineering managers** who need visible, incremental migration progress to report to stakeholders, rather than a rewrite that's perpetually "in progress" with nothing shipped.
- **Developers** implementing the routing façade and shadow-mode comparison who need a concrete pattern for gradual, rollback-safe cutover.

## How it helps

- Orders migration pieces by risk and coupling, starting with low-stakes, clearly-bounded functionality rather than the riskiest legacy code first.
- Introduces shadow-mode comparison for high-stakes logic, catching correctness regressions before they ever reach real users.
- Insists on actually decommissioning legacy code paths per piece, rather than letting old and new implementations coexist indefinitely.
- Walks through a full worked example: a 15-year-old billing system migrated piece by piece after a prior big-bang rewrite attempt stalled, including a shadow-mode diffing step and a tracked progress metric.

## How to use it

```bash
mkdir -p .claude/skills
cp -r strangler-fig-migration-pattern .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/strangler-fig-migration-pattern
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/strangler-fig-migration-pattern /path/to/your/project/.claude/skills/
```
