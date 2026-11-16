---
name: backward-compatibility-strategy
description: "Designs API contract and data schema changes so old clients, consumers, and readers keep working unmodified while new ones roll out — using additive-only changes, dual-write/dual-read windows, and compatibility-mode enforcement in CI, rather than coordinating a flag-day cutover across every consumer. Use when changing a shared API contract, a database schema touched by multiple services, or an event schema with independently-deployed consumers."
---

# Backward Compatibility Strategy

The old contract and the new contract must both work correctly at the same time, for the entire span of an uncoordinated rollout — because you cannot force every client, consumer, or service to redeploy in the same instant, and any strategy that assumes you can is really just an outage waiting for a slow deployer. Design every change as additive-first and prove compatibility mechanically (schema-diff in CI), not by review-time judgment call.

## When to use this

- A database column, table, or message schema is read by more than one service or deployable, and they can't all redeploy atomically.
- An API response shape needs to change and some consumers are third parties, mobile apps, or otherwise not on the team's release schedule.
- A migration must roll out gradually (canary, phased rollout, blue/green) and old and new code paths will run concurrently against the same data store.
- A schema-registry compatibility check (Kafka/Avro, Protobuf, JSON Schema) is failing in CI and the team needs to know which kind of change it's flagging and why.
- A rollback needs to be possible after a deploy — which requires the new schema/contract to still be readable by the old code being rolled back to.
- Two teams are debating whether a field rename, type change, or removal is "safe" and need a rule, not an opinion.

## Workflow

1. **Classify the change as additive, additive-with-default, or destructive before touching anything.** Additive (new optional field, new endpoint, new enum value consumers are required to ignore-unknown) is always safe. Additive-with-default (new required field with a server-supplied default for old writers) is safe if every reader tolerates the default. Destructive (remove/rename a field, tighten a type, change semantics of an existing field, remove an enum value) is never safe to do in one step — it always needs the multi-phase rollout in steps 2–7.
2. **Turn every destructive change into a multi-phase additive sequence — the expand/contract pattern.** Expand: add the new field/column/shape alongside the old one, without removing anything. Migrate: backfill existing data into the new shape, and switch writers to write both old and new (dual-write) or write new only if reads are already updated. Contract: once all readers are confirmed migrated (see step 8), remove the old field/column — this is the only step that is actually destructive, and it happens last, deliberately, after verification.
3. **For database schema changes, sequence column/table changes so every intermediate state is valid for both old and new application code.** Adding a `NOT NULL` column requires either a default value or a dual-phase migration (add nullable → backfill → add constraint) because the old code deploying concurrently doesn't know to populate it. Renaming a column is really "add new column, dual-write, backfill, migrate reads, drop old column" — never a single `ALTER ... RENAME` in a system with concurrent deployments.
4. **For API responses, never remove or repurpose a field readers might depend on, even undocumented ones.** Consumers read fields you didn't intend for them to use (Hyrum's Law); removing an "unused" field is a bet you can't verify without usage telemetry. Add new fields; mark old ones deprecated with a removal date communicated per `api-versioning-strategy`, and only actually remove after the version/consumer using it is confirmed gone.
5. **For message/event schemas, enforce compatibility mode mechanically via a schema registry, not code review.** Configure `BACKWARD` compatibility (new schema can read data written by the old schema — safe for rolling consumer upgrades) or `FORWARD` (old schema can read data written by the new schema — safe for rolling producer upgrades) or `FULL` (both) in Confluent Schema Registry / AWS Glue Schema Registry, and fail CI on any schema change that violates the configured mode rather than trusting the PR author caught it.
6. **Default new fields to optional/nullable, never required, at the point of introduction.** A field can be tightened to required only after every writer is confirmed to populate it (verified via telemetry, not assumption) — introducing it as required breaks every writer that hasn't redeployed yet, which is the single most common backward-compatibility bug.
7. **Design consumers to ignore unknown fields and unrecognized enum values rather than erroring.** This is a contract both directions must honor: producers can only add safely if consumers are guaranteed not to break on the addition. Document "ignore unrecognized fields/enum values" as an explicit consumer requirement, and add a test that feeds a message with an extra unknown field/enum value through the consumer to catch violations before production.
8. **Verify old-version usage with telemetry before the contract step.** Log schema version / API version per request or per message, and don't schedule the destructive contract phase until usage data shows zero (or a known, accepted-risk residual) traffic on the old shape — see `api-versioning-strategy` for the notification and sunset process this feeds.
9. **Test the exact rollback path, not just the forward migration.** After deploying the new schema/contract, confirm the previous application version can still run correctly against the current data shape — this is what makes rollback possible if the new deploy has an unrelated bug; a migration that isn't rollback-safe converts every future rollback decision into a forced-forward one.
10. **Treat dual-write windows as temporary and bounded, with an explicit close-out step.** A dual-write phase (writing both old and new shape) that never gets a scheduled end date becomes permanent extra write cost and a second source of truth that can drift; put a date and an owner on the contract phase when you open the expand phase, not after.
11. **Distinguish this from API versioning: versioning is for changes you can't make compatible; this strategy is for making as many changes as possible not need a version bump at all.** Reach for `api-versioning-strategy` only after confirming the change genuinely can't be done as an additive/expand-contract sequence — most "breaking" changes, on inspection, can.
12. **Run a compatibility gate in CI on every schema-touching PR.** Automate the classification from step 1 (schema-diff tooling: `buf breaking` for protobuf, Confluent Schema Registry compatibility check, or a custom JSON-Schema diff) so a destructive change is caught at PR time, not discovered when an old consumer starts erroring in production.

## Example

Scenario: Meridian Logistics runs a `shipment` Postgres table read by three independently-deployed services (tracking-api, billing-service, warehouse-sync) and written to a `shipment.updated` Kafka topic consumed by two more. The team needs to split a single `address` free-text column into structured `street`, `city`, `postal_code`, `country_code` columns.

Expand/contract sequence:

| Phase | Change | Old code | New code |
|---|---|---|---|
| Expand | Add nullable `street`, `city`, `postal_code`, `country_code` columns; add same fields (optional) to `shipment.updated` Avro schema, registered `BACKWARD` compatible | Reads/writes `address` only — unaffected | Deployed reading `address`, not yet writing structured fields |
| Migrate (dual-write) | New app code writes both `address` (derived from structured fields) and the structured fields; backfill job populates structured fields for existing rows from `address` | Still reads `address`, sees correct derived value | Reads structured fields when present, falls back to `address` |
| Verify | Telemetry confirms 100% of `shipment.updated` consumers (tracking-api, billing-service via schema-registry version metrics) are on the version reading structured fields | — | — |
| Contract | Stop writing `address`; after one release cycle with zero read traffic on `address` (confirmed via query logging), drop the column and remove it from the Avro schema | N/A — retired | Reads structured fields only |

Rejected alternatives:

| Alternative | Rejected because |
|---|---|
| Single migration: `ALTER TABLE shipment DROP COLUMN address, ADD COLUMN street ...` in one deploy | Any of the five services still running the old code (impossible to guarantee simultaneous deploy across independently-owned services) breaks immediately on the next read or write |
| Add structured fields as `NOT NULL` immediately | Old code deploying concurrently doesn't populate them, so every insert from an un-upgraded instance fails |
| Skip schema-registry compatibility enforcement, rely on PR review to catch breaking Avro changes | Compatibility violations in nested/optional Avro fields are easy to miss by eye; the registry check is what actually caught a reviewer-missed `country_code` type mismatch in this rollout |

## Common pitfalls

- **Making a new field required at introduction instead of optional-then-tightened.** Breaks every writer that hasn't redeployed yet — the single most common cause of "it worked in staging, broke in prod during rollout."
- **Treating "removed the field, nobody was in the code using it" as sufficient evidence.** Hyrum's Law: external and undocumented consumers use fields you didn't design for; only usage telemetry, not code search, tells you it's safe to remove.
- **Single-step destructive migrations (rename, drop, retype) in a system with independently deployed readers/writers.** Skips the expand/migrate/verify/contract sequence and guarantees a window where some deployed code is incompatible with the data shape.
- **Dual-write phases with no scheduled end date.** Becomes permanent, doubling write cost and creating a second source of truth that silently drifts.
- **Enforcing compatibility by code review instead of a schema-registry or schema-diff CI gate.** Reviewers miss compatibility violations in nested or optional fields; automate the check.
- **Consumers that error on an unrecognized field or enum value instead of ignoring it.** Silently defeats every additive change producers make, turning "safe" additions into breaking ones for that consumer.
- **Not testing the rollback path — only the forward migration.** A migration that isn't verified rollback-safe forecloses rolling back an unrelated bad deploy without a forced second migration.

## Output

A compatibility plan per schema/contract change (commonly a section in the migration PR description or `docs/architecture/migrations/<change-name>.md`) specifying: the change classification (additive/additive-with-default/destructive), the expand/migrate/verify/contract phases with owners and dates, the compatibility mode enforced in the schema registry or CI gate, and the telemetry signal that gates the contract phase — feeding into the CI schema-diff check, the rollback runbook, and (when the change can't be made compatible at all) the versioning and deprecation process in `api-versioning-strategy`.
