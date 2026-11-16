# Monolith-to-Microservices Migration Plan

A Claude Code skill for sequencing an incremental, low-risk migration from a monolith to services — with concrete milestones, rollback points, and a risk-ordered extraction plan instead of a big-bang rewrite.

## Who it helps

- **Tech leads and architects** turning an approved target service-decomposition into an actual, executable roadmap.
- **Engineering managers** who need to answer "how long will this take and what ships along the way" with real milestones rather than a diagram.
- **Developers** doing the extraction work who need clear rollback triggers and a safe cutover sequence for each service.

## How it helps

- Provides a risk/value ranking method for choosing extraction order, including picking a low-risk "pathfinder" service to prove out tooling first.
- Gives a full worked 30-week roadmap for a real-sounding retail monolith, including specific rollback triggers and dual-write end dates.
- Calls out the "boring middle" stall pattern — migrations that die after the first extraction because ongoing capacity wasn't reserved — and how to avoid it.
- Ties into companion skills (`strangler-fig-migration-pattern`, `cdc-change-data-capture-design`, `circuit-breaker-pattern-design`) for the mechanics of each phase.

## How to use it

```bash
mkdir -p .claude/skills
cp -r monolith-to-microservices-migration-plan .claude/skills/
```

Or place it under `~/.claude/skills/` for global use, or install via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/monolith-to-microservices-migration-plan
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/monolith-to-microservices-migration-plan /path/to/your/project/.claude/skills/
```
