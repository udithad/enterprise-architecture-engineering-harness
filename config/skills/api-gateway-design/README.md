# API Gateway Design

A Claude Code skill for designing the single entry point that fronts a set of backend services — routing, auth, rate limiting, aggregation, and protocol translation — instead of every client hard-coding service topology or every service duplicating cross-cutting concerns.

## Who it helps

- **Architects and tech leads** deciding between a single edge gateway, a Backend-for-Frontend per client type, or both, and drawing the line between gateway concerns and service-owned concerns.
- **Engineering managers** who need cross-cutting policy (auth, rate limits, versioning) enforced consistently instead of drifting across a dozen independently-maintained services.
- **Developers** building gateway routes or aggregation endpoints who need a concrete pattern for partial-failure handling and for keeping business logic out of the gateway layer.

## How it helps

- Frames the core decision — single edge gateway vs. BFF-per-client vs. both — around whether client needs have actually diverged, not as a default architectural layer.
- Gives an explicit checklist for what belongs in the gateway (TLS, coarse auth, rate limiting, routing) versus what must stay in services (fine-grained authorization, business logic).
- Walks through a full worked example: a logistics platform splitting one gateway into web/mobile/partner BFFs, including a mobile aggregation endpoint that cuts 6 round trips to 1 and an explicit partial-failure fallback.
- Distinguishes gateway responsibilities (north-south traffic) from service-mesh responsibilities (east-west traffic), cross-referencing `service-mesh-evaluation` so the two layers don't duplicate the same concern.

## How to use it

```bash
mkdir -p .claude/skills
cp -r api-gateway-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/api-gateway-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/api-gateway-design /path/to/your/project/.claude/skills/
```
