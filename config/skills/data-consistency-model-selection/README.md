# Data Consistency Model Selection

A Claude Code skill for choosing the right consistency model — strong, eventual, causal, read-your-writes — per use case instead of defaulting to one model everywhere and either overpaying in latency/availability or under-guaranteeing correctness where it actually matters.

## Who it helps

- **Architects and tech leads** designing distributed data access, replication, or caching who need to make the CAP/PACELC trade-off an explicit, per-data-type decision.
- **Engineering managers** investigating a bug where a user saw "impossible" data, traceable to an unstated consistency assumption nobody wrote down.
- **Developers** building against a datastore or API who need to know the real consistency guarantee instead of assuming strong consistency by default.

## How it helps

- Classifies data by actual correctness requirement first, then names the specific guarantee needed (strong, causal, read-your-writes, bounded staleness) instead of a vague strong/eventual binary.
- Walks through a full worked example: a fintech app splitting account balances (strongly consistent) from an activity feed (eventually consistent with a stated staleness bound and read-your-writes), and catching a downstream service that had silently assumed the wrong guarantee.
- Makes conflict resolution (last-write-wins, CRDTs, application-level merge) a deliberate choice instead of a default that silently drops data.
- Insists every consistency guarantee be documented and tested under real partition conditions, not just assumed from the happy path.

## How to use it

```bash
mkdir -p .claude/skills
cp -r data-consistency-model-selection .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/data-consistency-model-selection
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/data-consistency-model-selection /path/to/your/project/.claude/skills/
```
