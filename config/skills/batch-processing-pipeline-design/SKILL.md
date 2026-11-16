---
name: batch-processing-pipeline-design
description: "Architects batch data/job pipelines with checkpointing, idempotent reprocessing, and failure isolation so a partial failure doesn't corrupt output or force a full rerun from scratch. Use when designing nightly ETL jobs, bulk reporting, large-scale data migrations, or any workload processed in scheduled chunks rather than in real time."
---

# Batch Processing Pipeline Design

Design batch jobs so a failure partway through is cheap to recover from — reprocessing only what's needed, never silently double-applying an already-completed step, and never leaving downstream consumers with half-written output.

## When to use this

- A nightly ETL or reporting job currently reruns from scratch on any failure, wasting hours and delaying downstream consumers who need the data by a fixed SLA (e.g., "reports ready by 6am").
- A batch job has grown multiple stages (extract, transform, validate, load) and a failure in stage 3 currently loses the work already done in stages 1-2.
- The same batch job is at risk of running twice (a scheduler retry racing a still-running job) and there's no protection against double-processing records.
- A large one-time data migration or backfill needs to run over millions of records without holding a lock, a transaction, or memory for the entire dataset at once.
- Downstream consumers occasionally see partial or inconsistent output because the batch job writes results incrementally with no atomic "publish" step.

## Workflow

1. **Break the pipeline into discrete, independently retryable stages** (extract, validate, transform, load) with a defined contract between them — the output of one stage should be a durable artifact (a file, a staging table) the next stage reads, not an in-memory handoff that's lost if the process crashes.
2. **Design each stage to be idempotent**: reprocessing the same input chunk produces the same output, not duplicated rows. Use natural keys with upsert semantics, or delete-then-insert per partition, rather than blind append (see `idempotency-design-for-apis` for the same principle applied to job chunks instead of API calls).
3. **Checkpoint progress explicitly** — record which chunks/partitions have completed successfully (a `job_run_id` + `partition` + `status` table, or a manifest file) so a restart resumes from the last good checkpoint instead of reprocessing everything or, worse, skipping unprocessed data.
4. **Partition the workload for parallelism and failure isolation** (by date, tenant, or hash range) so one bad partition (malformed data, a poison record) fails and is retried independently without blocking or corrupting the other 999 partitions.
5. **Isolate poison records** with a dead-letter mechanism at the record level, not just the partition level — a job that aborts an entire day's partition because of one malformed row wastes far more reprocessing than isolating that one row and continuing.
6. **Write output behind an atomic publish step.** Load into a staging table/location, validate row counts and key invariants, then atomically swap/rename into the location consumers read (or use a versioned pointer) — never let partial output become visible mid-run.
7. **Set and monitor an SLA per pipeline stage**, not just the pipeline as a whole, so a slow extract stage that's about to blow the overall deadline is visible early enough to act (see `capacity-planning-and-scaling-model` for sizing the compute to hit the SLA).
8. **Design explicit backfill/replay tooling** from day one — the ability to reprocess a specific historical date range on demand (for a late-arriving source correction, or a bug fix) using the same idempotent stages, not a separate one-off script.
9. **Decide the concurrency-safety story for overlapping runs.** Use a distributed lock or a `job_run` status check so a scheduler retry or manual kick-off can't start a second run over the same partition while one is already in flight.
10. **Instrument row counts, duration, and data-quality checks per stage** and alert on drift (row count deviates >X% from the historical baseline) — silent data-quality regressions are far more dangerous in batch than outright failures, because nothing crashes to draw attention to them.

## Example

Scenario: `RetailPulse` runs a nightly pipeline that ingests point-of-sale data from 3,000 stores, aggregates daily sales, and loads results into a reporting warehouse by 6am for regional managers. Today, a single malformed record from one store aborts the entire job, and a restart reprocesses all 3,000 stores from scratch, sometimes missing the 6am SLA.

Redesign:
- **Partitioning**: one partition per store per day (3,000 partitions), processed in parallel with bounded concurrency (200 workers).
- **Checkpointing**: a `pipeline_runs(run_date, store_id, stage, status, row_count)` table tracks each partition's progress through extract → validate → aggregate → load.
- **Poison isolation**: a malformed record within one store's file is routed to a dead-letter location with the parse error; that store's partition completes with the other valid records and a `data_quality_flag`, instead of aborting.
- **Idempotency**: the load stage upserts on `(store_id, sale_date)`, so retrying a partition after a transient warehouse timeout never double-counts sales.
- **Atomic publish**: aggregated results land in `reporting.daily_sales_staging`; after all 3,000 partitions report `status=complete` and a row-count sanity check passes (total ≈ prior-day ±15%), a single `RENAME TABLE` swaps staging into `reporting.daily_sales`, so managers never see a partially-loaded table.
- **Backfill**: the same pipeline accepts a `--replay-date` flag reusing all the same idempotent stages, used when a store's POS system sends a late correction three days later.

Result: a single store's malformed record no longer takes down the whole run; a restart after a transient failure resumes from checkpoint instead of reprocessing 3,000 stores, and the 6am SLA is met even on partial-failure nights.

## Common pitfalls

- **No checkpointing, so any failure means a full rerun** — expensive in both compute and the time lost against a fixed SLA.
- **Non-idempotent load stages** that double-count or duplicate rows when a retry reprocesses a chunk that had actually already partially succeeded.
- **One poison record aborting an entire partition or the whole job**, instead of being isolated to a dead-letter path.
- **No atomic publish step**, so downstream consumers occasionally read a half-loaded table mid-run.
- **No protection against concurrent/overlapping runs**, letting a scheduler retry race an already-running job and corrupt shared output.
- **Treating backfill as a one-off manual script** instead of reusing the pipeline's own idempotent stages, causing backfills to drift from what production actually does.
- **Monitoring only "did the job succeed or fail"** with no row-count or data-quality drift check, missing silent corruption that doesn't crash anything.

## Output

A batch pipeline design doc (e.g., `docs/architecture/batch-pipeline-<name>.md`) specifying stage boundaries and contracts, partitioning scheme, checkpoint/idempotency mechanism, poison-record handling, the atomic publish step, concurrency-safety approach, and backfill tooling — feeding into the pipeline's implementation and its per-stage SLA monitors.
