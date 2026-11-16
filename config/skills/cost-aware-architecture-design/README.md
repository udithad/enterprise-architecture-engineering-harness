# Cost-Aware Architecture Design

A Claude Code skill for treating cloud cost as a first-class architectural constraint — modeled per unit of work and traded off explicitly against latency, availability, and scalability — instead of discovering it in next month's bill.

## Who it helps

- **Architects and tech leads** comparing architectural options (serverless vs. always-on, single- vs. multi-region) that have meaningfully different cost curves nobody has compared explicitly.
- **Engineering managers** who need a real cost-per-customer or cost-per-transaction figure for finance, and a budget/drift alert tied to it.
- **Developers** tracing an unexplained cost spike back to an architectural cause — an unbounded retry loop, over-provisioned capacity, or unmodeled cross-region data transfer.

## How it helps

- Forces a cost unit (per request, per user, per transaction) so decisions are traceable, not just an aggregate bill that's hard to reason about at design time.
- Compares options on cost curves across a utilization range rather than a single snapshot number, surfacing crossover points where the cheaper option flips.
- Walks through a full worked example: a video-transcoding platform's always-on vs. serverless vs. managed-service cost comparison, including a documented crossover trigger for revisiting the decision.
- Connects resilience patterns (retries, autoscaling) to cost risk, not just availability risk — a retry storm is a cost incident too.

## How to use it

```bash
mkdir -p .claude/skills
cp -r cost-aware-architecture-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/cost-aware-architecture-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/cost-aware-architecture-design /path/to/your/project/.claude/skills/
```
