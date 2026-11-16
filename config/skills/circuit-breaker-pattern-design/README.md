# Circuit Breaker Pattern Design

A Claude Code skill for wrapping calls to unreliable dependencies in a trip/half-open/reset circuit breaker, so a slow or failing downstream service degrades gracefully instead of exhausting the caller's own resources and cascading into a wider outage.

## Who it helps

- **Architects and tech leads** designing resiliency around a dependency with a track record of cascading failures, deciding trip thresholds and fallback behavior up front.
- **Engineering managers** who've seen a "minor" downstream degradation turn into a full outage because nothing cut off the retry storm.
- **Developers** implementing the breaker who need concrete guidance on trip criteria, cooldown sizing, bulkhead pairing, and what the Open-state fallback should actually do.

## How it helps

- Defines the three breaker states and their transition rules precisely, including which failures should (and shouldn't, like 4xx errors) count toward tripping.
- Pairs the breaker with a bulkhead so caller resources aren't already exhausted before the breaker's failure count catches up.
- Walks through a full worked example: a reservation service breaking calls to a third-party pricing API per region, with a cached-price fallback and a chaos-tested recovery path.
- Flags the common trap of a Half-Open state that floods the recovering dependency with a full retry instead of a small bounded probe.

## How to use it

```bash
mkdir -p .claude/skills
cp -r circuit-breaker-pattern-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/circuit-breaker-pattern-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/circuit-breaker-pattern-design /path/to/your/project/.claude/skills/
```
