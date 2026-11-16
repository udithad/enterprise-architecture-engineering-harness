# Serverless Architecture Fit Assessment

A Claude Code skill for assessing whether FaaS/serverless actually fits a given workload's traffic shape, latency needs, and cost curve — instead of adopting it by trend or rejecting it by default.

## Who it helps

- **Architects and tech leads** choosing a compute model for a new workload, who need the traffic-shape-to-serverless-fit trade-off made explicit and cost-modeled rather than assumed.
- **Engineering managers** facing an unexpected serverless cost increase who need to know whether the workload has simply outgrown the model's sweet spot.
- **Developers** hitting cold-start latency issues who need a framework for deciding between mitigation (provisioned concurrency) and migrating to a different compute model.

## How it helps

- Gives a concrete classification (steady vs. bursty vs. infrequent traffic) that determines whether serverless's pay-per-invocation economics actually win.
- Forces a real cost-curve comparison against reserved capacity at current and projected scale, instead of assuming serverless is always cheaper.
- Covers the hard limits (execution duration, statelessness) that rule serverless out regardless of cost, and the hidden operational costs (lock-in, observability, testing) most evaluations skip.
- Walks through a full worked example: an analytics SaaS choosing serverless for bursty PDF generation but reserved-capacity batch compute for a 45-minute nightly aggregation job, with modeled costs for each.

## How to use it

```bash
mkdir -p .claude/skills
cp -r serverless-architecture-fit-assessment .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/serverless-architecture-fit-assessment
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/serverless-architecture-fit-assessment /path/to/your/project/.claude/skills/
```
