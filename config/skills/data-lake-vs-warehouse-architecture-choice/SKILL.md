---
name: data-lake-vs-warehouse-architecture-choice
description: "Decides between (or how to combine) a data lake and a data warehouse based on data shape, query patterns, schema stability, and consumer needs, instead of defaulting to whichever pattern is currently trendy. Use when designing analytical/reporting infrastructure or when an existing lake or warehouse is being misused for the other's job."
---

# Data Lake vs. Warehouse Architecture Choice

Choose between a data lake, a data warehouse, or a lakehouse/hybrid combination based on what the data actually looks like and how it needs to be queried — schema-on-read vs. schema-on-write, raw/semi-structured vs. curated/structured, exploratory vs. BI/reporting consumers — instead of picking whichever pattern is trending or defaulting to "we already have a warehouse, put everything there."

## When to use this

- A team is designing analytical infrastructure from scratch and needs to decide the foundational storage/query pattern before choosing specific tools.
- An existing data warehouse is being used to store raw, semi-structured, or rapidly-evolving-schema data (logs, clickstream, ML training data), causing brittle ETL and constant schema-migration pain.
- An existing data lake has become a "data swamp" — ungoverned raw files with no consistent schema, catalog, or quality guarantees — and BI/reporting consumers can't reliably query it.
- Multiple data-consumer types (data scientists needing raw/flexible access, analysts needing curated/governed tables, applications needing low-latency lookups) are all being forced through one ill-fitting pattern.
- Cost or performance complaints about the current analytical stack trace back to a structural mismatch between data shape and storage pattern, not just a tuning problem.

## Workflow

1. **Characterize the data by shape and change rate**: structured and schema-stable (transactional exports, well-defined events) suits a warehouse; semi-structured, high-volume, or schema-evolving (raw logs, clickstream, IoT, unstructured text/images, ML features) suits a lake. Most real organizations have both, and forcing one pattern to serve both shapes is the root cause of most lake/warehouse pain.
2. **Characterize consumers and their query patterns**: BI dashboards and analysts running structured SQL against known schemas want a warehouse's performance and governance. Data scientists exploring raw data, building ML features, or needing formats other than SQL tables want a lake's flexibility and lower ingestion friction (schema-on-read, no upfront transformation cost).
3. **Decide schema-on-write vs. schema-on-read deliberately.** Schema-on-write (warehouse) catches data-quality problems at ingestion and gives consumers a stable, governed contract, at the cost of ETL rigidity and slower onboarding of new data sources. Schema-on-read (lake) ingests raw data fast and defers structure to query time, at the cost of pushing data-quality problems downstream to every consumer who queries it.
4. **Consider a lakehouse pattern (lake storage + warehouse-like transactional/schema layer, e.g., Delta Lake/Iceberg/Hudi on object storage) when you need both**: cheap, flexible raw storage AND ACID transactions, schema enforcement, and performant SQL on curated layers — increasingly the default choice for new builds rather than a hard either/or.
5. **Design a layered architecture even within a single pattern**: raw/bronze (as-ingested, minimally transformed) → cleaned/silver (validated, deduplicated, conformed) → curated/gold (business-level aggregates, BI-ready) — this applies whether you land on a pure lake, pure warehouse, or lakehouse, and prevents raw data and BI-ready data from being tangled in the same layer.
6. **Plan governance and cataloging from day one for a lake**, since schema-on-read means nothing stops raw data from silently drifting or becoming undocumented — a data catalog (schema registry, lineage tracking) is not optional infrastructure for a lake the way it can feel optional for a rigidly-schematized warehouse.
7. **Evaluate cost profile explicitly**: object storage underlying a lake is typically far cheaper per TB than warehouse-native storage, but warehouse compute is often more efficient per query for structured, repeated BI workloads — model cost per query pattern, not just per TB stored (see `cost-aware-architecture-design`).
8. **Match access control and compliance requirements to the pattern** — fine-grained row/column-level security and audit logging are typically more mature in warehouse platforms; a lake holding regulated data needs an explicit plan (`data-privacy-and-pii-handling-review`) rather than assuming the same controls transfer automatically.
9. **Design the ETL/ELT pipeline direction to match the pattern** — ELT (load raw, transform in place) fits lake/lakehouse patterns with cheap storage and powerful in-place compute; traditional ETL (transform before load) fits a warehouse where storage is comparatively expensive and schema is enforced at the door (see `etl-pipeline-design`, `elt-pipeline-design-with-modern-tools`).
10. **Avoid re-litigating the choice per new data source.** Once the layered pattern (bronze/silver/gold or equivalent) is established, new data sources should have a default landing zone and promotion path defined, rather than each new source triggering its own lake-vs-warehouse debate.

## Example

Scenario: `Insightly Analytics` has a Snowflake data warehouse serving BI dashboards, but the data science team has started dumping raw clickstream JSON and third-party API responses into warehouse staging tables, causing constant schema-break incidents whenever an upstream API changes its response shape, and driving up warehouse storage cost for data that's queried maybe once a month.

Redesign (lakehouse pattern):
- **Bronze (S3 + Iceberg)**: raw clickstream JSON and API responses land as-is, partitioned by ingestion date, schema-on-read — no upfront transformation, so upstream schema changes no longer break ingestion.
- **Silver (S3 + Iceberg, Spark transforms)**: cleaned, deduplicated, conformed-schema tables (e.g., `clickstream_events` with a stable column set), with a schema registry enforcing compatibility rules on writes to this layer — catching malformed data before it reaches analysts.
- **Gold (Snowflake, loaded from silver via scheduled ELT)**: business-level aggregates (daily active users, funnel conversion) that BI dashboards query directly — Snowflake now holds only curated, stable-schema data, and its storage/compute cost drops because raw and rarely-queried data live in cheap object storage instead.
- **Data scientists** query bronze/silver directly via Spark/Athena for exploratory work and ML feature engineering, without needing curated gold-layer aggregates or warehouse compute credits for ad hoc exploration.
- **Governance**: a data catalog (schema registry + column-level lineage) tracks bronze → silver → gold transformations, so a downstream schema-break question ("why did `clickstream_events.country` become null") is traceable to the exact upstream change.

Result: warehouse storage cost drops ~40% (raw data moved to cheaper object storage), upstream API schema changes no longer break the whole pipeline (they're isolated to bronze, with silver enforcing the contract), and data scientists get flexible raw access without contending with analysts for warehouse compute.

## Common pitfalls

- **Forcing raw, rapidly-evolving-schema data into a warehouse**, causing brittle ETL that breaks on every upstream schema change.
- **Dumping everything into a lake with no governance/catalog**, producing a "data swamp" that BI consumers can't trust or query reliably.
- **Treating lake vs. warehouse as a permanent, one-time choice** instead of a per-data-type and per-consumer decision that can (and often should) result in both, connected by a layered pipeline.
- **No layered (bronze/silver/gold) structure**, tangling raw and BI-ready data in the same tables regardless of which underlying pattern is chosen.
- **Comparing cost per TB stored only**, missing that warehouse compute efficiency for repeated structured queries can outweigh a lake's cheaper raw storage for BI workloads.
- **Applying warehouse-grade access controls assumptions to a lake holding regulated data** without verifying the lake platform actually enforces equivalent row/column-level security.

## Output

A data platform architecture doc (e.g., `docs/architecture/data-platform.md`) stating the chosen pattern (lake, warehouse, or lakehouse) per data domain, the layered pipeline structure (bronze/silver/gold or equivalent), the ELT/ETL direction, governance/catalog plan, and a cost model comparing storage and compute across the query patterns actually expected — feeding into pipeline implementation and platform tooling choices.
