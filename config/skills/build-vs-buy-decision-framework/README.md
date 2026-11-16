# Build vs. Buy Decision Framework

A Claude Code skill for scoring build-vs-buy options against total cost of ownership, differentiation value, control, and time-to-market — producing a defensible written recommendation instead of a gut call made in a Slack thread.

## Who it helps

- **Architects and tech leads** deciding whether to build a capability in-house or adopt a vendor/OSS solution, and who need the rationale to hold up to later scrutiny.
- **Engineering managers** who need a repeatable, written framework instead of re-litigating the same build-vs-buy argument for every new capability.
- **Developers** contributing to the evaluation who need to understand which criteria actually matter (differentiation, full-horizon cost, reversibility) versus which are noise.

## How it helps

- Separates "core differentiator" from "undifferentiated plumbing" as the first cut, before any scoring happens.
- Forces both build and buy costs onto the same time horizon, including the maintenance and organizational-ownership costs that are usually left out of "build" estimates.
- Walks through a full worked example: a notification-system build-vs-buy decision with a weighted scoring table and a named, calculated revisit trigger.
- Ends every decision with an explicit trigger for re-evaluation, not just a calendar date, so the decision doesn't silently go stale.

## How to use it

```bash
mkdir -p .claude/skills
cp -r build-vs-buy-decision-framework .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/build-vs-buy-decision-framework
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/build-vs-buy-decision-framework /path/to/your/project/.claude/skills/
```
