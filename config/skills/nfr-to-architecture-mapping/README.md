# NFR to Architecture Mapping

A Claude Code skill for translating non-functional requirements — availability, latency, throughput, security, compliance — into concrete, traceable architectural decisions, instead of leaving them as vague adjectives nobody actually designed against.

## Who it helps

- **Architects and tech leads** who need to show, line by line, how each stated NFR is satisfied by a specific component or pattern before a design review.
- **Engineering managers** who need NFR conflicts (e.g., availability vs. budget) surfaced and explicitly signed off on, rather than silently resolved one way and discovered later.
- **Developers** implementing a design who need a clear link between "why does this exist" and the NFR it satisfies, instead of cargo-culting patterns without a stated reason.

## How it helps

- Forces every NFR into a numeric, testable statement before mapping it to architecture — "fast" and "scalable" aren't actionable inputs.
- Produces a traceability table (NFR → decision → component → verification) that makes NFR satisfaction auditable instead of assumed.
- Surfaces conflicts between NFRs (consistency vs. latency, availability vs. cost) explicitly, with the trade-off decision documented rather than buried.
- Walks through a full worked example: an internal approval system mapping five NFRs (availability, latency, concurrency, compliance, cost) to specific components, including a documented trade-off where budget ruled out multi-region DR.

## How to use it

```bash
mkdir -p .claude/skills
cp -r nfr-to-architecture-mapping .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/nfr-to-architecture-mapping
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/nfr-to-architecture-mapping /path/to/your/project/.claude/skills/
```
