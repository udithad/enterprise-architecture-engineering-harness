# Caching Layer Architecture

A Claude Code skill for designing multi-tier caching — client, CDN, application, database — with an explicit invalidation strategy per tier, so caching speeds the system up without introducing stale or inconsistent reads.

## Who it helps

- **Architects and tech leads** designing a caching layer from scratch, or untangling an ad hoc cache that's causing staleness bugs nobody can trace.
- **Engineering managers** who need database load and latency under control without trading it for a new class of data-correctness incident.
- **Developers** implementing a cache who need a concrete pattern for invalidation, key design, stampede protection, and graceful degradation on cache failure.

## How it helps

- Classifies data by change frequency first, then matches it to the right pattern (cache-aside, write-through, write-behind) and tier instead of defaulting to one cache everywhere.
- Makes invalidation strategy an explicit, per-entity decision (TTL-only, write-time, or event-driven) instead of an assumed default nobody actually chose.
- Walks through a full worked example: a product catalog cutting origin load 85% and p95 latency from 240ms to 35ms with a CDN + Redis + event-driven invalidation design.
- Covers the failure modes that matter most — cache stampede, cache-as-hard-dependency, key collisions across tenants — with concrete mitigations for each.

## How to use it

```bash
mkdir -p .claude/skills
cp -r caching-layer-architecture .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/caching-layer-architecture
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/caching-layer-architecture /path/to/your/project/.claude/skills/
```
