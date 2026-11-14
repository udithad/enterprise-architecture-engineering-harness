# ADR-0004: Persistence: SQLite via SQLx with serialized IMMEDIATE write transactions

| Field | Value |
|---|---|
| Status | Accepted (D-01) |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
Phase 1 needs durable workflow state, audit, outbox and registries without external infrastructure, with a path to PostgreSQL.

## Decision
* SQLite (WAL, `foreign_keys=ON`, `busy_timeout=10s`) through `eaeh-store::Db`; ordered, embedded migrations (`crates/eaeh-store/migrations`).
* `Db::write()` is the **only** write path: an in-process mutex plus `BEGIN IMMEDIATE`, so multi-statement invariants (optimistic version checks, gapless audit sequence) are deterministic and `SQLITE_BUSY_SNAPSHOT` cannot occur. (A concurrency defect caused by three writes bypassing this path was found by the end-to-end suite and fixed; a fitness test now prevents regressions.)
* State change + audit record + outbox event are committed in one transaction.
* Domain crates own their SQL against the shared schema behind repository-style APIs (`ApprovalManager`, `AgentRegistry`, `PromptRegistry`, `WorkflowEngine`, …), keeping SQL out of callers.
* Database file `0600`; EAEH-created data directory `0700`.

## Consequences
Single-writer throughput is adequate for Phase 1. A PostgreSQL adapter requires porting SQLite-specific SQL in domain crates; there is no storage trait per repository yet (technical debt TD-01).

## Alternatives considered
Trait-per-repository with in-memory + SQLite implementations (rejected for Phase 1: duplicates code without a second backend).
