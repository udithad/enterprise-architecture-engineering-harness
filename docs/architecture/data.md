# Data Model and Persistence

Schema: `crates/eaeh-store/migrations/0001_initial.sql` (owned by `eaeh-store`; SQL per
domain crate). Conventions: TEXT ids (UUIDv7 or validated slugs), RFC3339 UTC microsecond
timestamps (lexicographically sortable), JSON documents as TEXT, foreign keys enforced.

| Table | Owner crate | Purpose | Lifecycle / integrity |
|---|---|---|---|
| `tenants`, `projects` | governance | Tenancy | Created by bootstrap / admin |
| `principals`, `api_tokens` | governance | Identity, credentials | Token hash unique; revoke/rotate; principal enable/disable |
| `audit_log` | store | Append-only hash chain | Gapless `seq`; verified by `verify_audit_chain` |
| `outbox`, `event_deliveries` | events | Durable events, per-consumer delivery | `event_id` unique (dedupe); pending → done/dead |
| `policies` | governance | Persisted policy documents | Immutable per version; one active per id |
| `approvals`, `approval_decisions` | governance | Gates and decisions | pending → approved/rejected/superseded; decision once |
| `budget_usage` | governance | Budget limits and usage per owner | Conditional atomic increments |
| `plugins` | extensibility | Manifests, digests, state, grants | Immutable per version; lifecycle state machine |
| `prompt_templates` | intelligence | Versioned prompts | One approved per id |
| `model_usage` | intelligence | Model/provider/version/tokens/cost/prompts/context per call | Append-only |
| `context_snapshots` | intelligence | Provenance manifest of assembled contexts | Keyed by hash |
| `agent_definitions`, `agent_instances`, `agent_messages` | agents | Registry, instances (lineage, depth), delegation messages | Definitions immutable per version |
| `workflow_definitions`, `workflow_instances`, `workflow_transitions`, `tasks` | workflow | Definitions, durable state (optimistic `version`), checkpoints, idempotent tasks | Transitions append-only with checkpoints; `tasks.idempotency_key` unique |
| `workspaces`, `workspace_locks`, `artifacts` | workspace | Workspaces, locks, artifact metadata (blobs on disk, content-addressed per tenant) | Artifact versions unique per (tenant, project, name) |
| `idempotency_keys` | store | First-result-wins records | Insert-or-ignore |
| `knowledge_sources`, `knowledge_documents`, `knowledge_chunks`, `knowledge_chunks_fts` (FTS5), `knowledge_embeddings`, `knowledge_conflicts` | knowledge (Phase 2, migration 0002) | Governed knowledge, index, vectors, conflicts | Versions unique per (source, subject); delete purges content/FTS/vectors |
| `skills` | skills (Phase 2) | Skill versions, digest, status, approval | Immutable per version |
| `projects.domain`, `principals.teams` | governance (Phase 2) | ABAC attributes | Admin-managed, audited |
| `model_usage.skill_refs`, `model_usage.knowledge_refs` | intelligence (Phase 2) | Traceability of skills/knowledge per model call | Append-only |
| `approvals.status` `changes_requested`, `approval_decisions.decision` `request_changes` | governance (Phase 3, migration 0003 – tables rebuilt to widen CHECK constraints) | Revision decisions | As before |
| `trace_links` | workflow (Phase 3, migration 0003) | Traceability links of approved artifacts (artifact id/name/version/hash, approval, instance) | Insert-or-ignore; only on approval |

## Consistency and transactions

* All writes through `Db::write()` (in-process mutex + `BEGIN IMMEDIATE`; fitness-enforced).
* A state change, its audit record and its outbox event commit atomically.
* Workflow instance updates are optimistic (`WHERE version = ?`); concurrent modification
  fails with `conflict`.
* Artifact blobs are written (temp file + rename) before the metadata row; an orphan blob
  after a crash is harmless (content-addressed).

## Migrations, backup, restore

* Migrations are embedded and applied at startup (`sqlx::migrate!`); applied versions are
  reported by `/v1/health/details` and `eaeh health`.
* Migrations: `0001` (Phase 1), `0002_knowledge_skills` (Phase 2), `0003_engineering_design` (Phase 3). Changes after release must be new migrations.
* Backup: stop the server (or use `sqlite3 eaeh.db ".backup …"`), copy the data directory
  (`eaeh.db*`, `artifacts/`, `workspaces/`). Restore: copy back, run `eaeh audit verify`.
  Automated backup/DR is Phase 8 (A-19).

## Sensitive data

Token hashes only; audit details, task errors, event errors and context data are redacted;
prompts and tool inputs are not stored in audit (only `input_sha256`). Workspace and
artifact contents may contain project data and are protected by filesystem permissions
(no encryption at rest in Phase 1, A-15).
