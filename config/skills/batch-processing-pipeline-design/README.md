# Batch Processing Pipeline Design

A Claude Code skill for architecting batch data and job pipelines with checkpointing, idempotent reprocessing, and failure isolation — so a partial failure is cheap to recover from instead of forcing a full rerun or silently corrupting output.

## Who it helps

- **Architects and tech leads** designing nightly ETL, bulk reporting, or large-scale migration pipelines that need to hit a fixed delivery SLA even when something in the middle fails.
- **Engineering managers** tired of on-call pages for batch jobs that rerun from scratch on every transient failure, burning compute and blowing SLAs.
- **Developers** building the pipeline who need a concrete pattern for partitioning, checkpointing, poison-record isolation, and atomic publish.

## How it helps

- Breaks a batch pipeline into independently retryable, idempotent stages with a durable contract between them, instead of an all-or-nothing monolith run.
- Gives a concrete checkpointing and partitioning pattern so one bad record or one bad partition doesn't take down the whole job.
- Walks through a full worked example: a 3,000-store nightly sales pipeline redesigned with per-store partitioning, a dead-letter path for malformed records, and an atomic staging-to-production swap.
- Distinguishes backfill/replay tooling built on the same idempotent stages from a risky one-off script that drifts from production behavior.

## How to use it

```bash
mkdir -p .claude/skills
cp -r batch-processing-pipeline-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/batch-processing-pipeline-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/batch-processing-pipeline-design /path/to/your/project/.claude/skills/
```
