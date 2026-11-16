# Rate Limiting Strategy Design

A Claude Code skill for designing a rate-limiting strategy for an API or service — algorithm choice, enforcement placement, limiting dimensions, and distributed-correctness — instead of bolting on an ad hoc counter that races under concurrency.

## Who it helps

- **Architects and tech leads** deciding how and where to throttle traffic for a public API, partner integration, or a service that must respect a downstream's contractual quota.
- **Engineering managers** who need to justify tiered quota limits (free/pro/enterprise) to product and customer support with a defensible, documented policy.
- **Developers** debugging a limiter that under- or over-counts requests across replicas, or fielding disputes over what counts against a customer's quota.

## How it helps

- Walks through algorithm selection (token bucket, leaky bucket, fixed window, sliding window) with the actual trade-offs — burst tolerance vs memory cost vs boundary-burst risk — instead of picking one by default.
- Solves the distributed-correctness problem explicitly: why naive check-then-increment races across nodes, and how an atomic Redis-backed operation fixes it.
- Draws a hard line between rate limiting (proactive, protects a resource from volume) and circuit breaking (reactive, protects a caller from a failing downstream) so the two aren't implemented as the same mechanism.
- Includes a full worked example — a payments API with tiered per-key limits plus a separate global guard for a downstream contractual cap — with actual numbers, a request-path diagram, and rejected alternatives.

## How to use it

```bash
mkdir -p .claude/skills
cp -r rate-limiting-strategy-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/rate-limiting-strategy-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/rate-limiting-strategy-design /path/to/your/project/.claude/skills/
```
