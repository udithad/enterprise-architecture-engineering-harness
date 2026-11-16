# Capacity Planning and Scaling Model

A Claude Code skill for modeling expected load growth and turning it into concrete scaling triggers, headroom targets, and a per-tier scaling plan — so capacity decisions are made from data instead of discovered mid-outage.

## Who it helps

- **Architects and tech leads** preparing for a known growth event (launch, campaign, seasonal peak) who need to know exactly where the system breaks, not just whether it survives expected peak.
- **Engineering managers** who need a real answer to "how much runway do we have" instead of a shrug, and a budget attached to any planned scaling.
- **Developers** setting autoscaling thresholds who need triggers validated against actual breaking points and provisioning lead time, not guessed round numbers.

## How it helps

- Identifies the actual binding constraint per tier (which is often not CPU) instead of assuming uniform scalability across the whole stack.
- Distinguishes tiers that can autoscale from those that need pre-provisioned headroom and a manual review cadence.
- Walks through a full worked example: a ticketing platform's 40x on-sale spike, where load-testing to the real breaking point revealed the DB connection pool — not CPU — as the actual limit, leading to an admission-queue design instead of a wall of 5xx errors.
- Ties every scaling scenario to a cost estimate and a runbook, so temporary headroom doesn't become a permanent, unapproved bill.

## How to use it

```bash
mkdir -p .claude/skills
cp -r capacity-planning-and-scaling-model .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/capacity-planning-and-scaling-model
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/capacity-planning-and-scaling-model /path/to/your/project/.claude/skills/
```
