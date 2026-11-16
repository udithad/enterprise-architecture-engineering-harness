# Zero-Downtime Deployment Architecture

A Claude Code skill for architecting application, database, and traffic-routing changes so releases never require a maintenance window — staged backward-compatible schema migrations, rolling/blue-green deploys, and safe in-flight request handling.

## Who it helps

- **Architects and tech leads** designing a new system to be zero-downtime from day one, or retrofitting an existing system that currently relies on scheduled maintenance windows.
- **Engineering managers** who need release risk reduced structurally after an unplanned outage caused by an incompatible schema change or a version mismatch during rollout.
- **Developers** writing database migrations or API changes who need a concrete, repeatable staged pattern for renaming or restructuring data without breaking either the old or new code during a rolling deploy.

## How it helps

- Names the core constraint every zero-downtime design must satisfy: two application versions run simultaneously during every rolling deploy, and every change must be safe under that condition.
- Gives a concrete, reusable staged pattern for schema changes (additive → dual-write → backfill → cutover → cleanup) instead of one risky atomic migration.
- Covers the parts most teams miss: connection draining, long-running/stateful operations surviving a deploy, and async event schema versioning treated with the same rigor as synchronous APIs.
- Walks through a full worked example: renaming a field on an orders table via four safe, independently-deployable steps instead of a single maintenance-window migration.

## How to use it

```bash
mkdir -p .claude/skills
cp -r zero-downtime-deployment-architecture .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/zero-downtime-deployment-architecture
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/zero-downtime-deployment-architecture /path/to/your/project/.claude/skills/
```
