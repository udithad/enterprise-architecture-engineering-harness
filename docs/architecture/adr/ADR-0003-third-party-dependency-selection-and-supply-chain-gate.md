# ADR-0003: Third-party dependency selection and supply-chain gate

| Field | Value |
|---|---|
| Status | Accepted (one exception pending approval) |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
Dependencies must be evaluated for maturity, security, maintenance, licensing, compatibility and operational complexity (master §27). The repository licence is Apache-2.0.

## Decision
Pinned via `Cargo.lock` (CI uses `--locked`). Selected crates (MIT and/or Apache-2.0 unless noted):

| Crate | Version | Purpose | Evaluation notes |
|---|---|---|---|
| tokio / tokio-util | 1.54 / 0.7.20 | async runtime, cancellation | de-facto standard, Tokio project |
| axum / tower | 0.8.9 / 0.5.3 | HTTP API | Tokio project; minimal features enabled |
| sqlx (sqlite) | 0.9.0 | persistence, migrations | runtime-checked static SQL only (`SqlSafeStr`); MSRV 1.94 |
| serde / serde_json / toml | 1 / 1 / 1.2 | serialization, config | |
| sha2 | 0.11.1 | audit chain, digests | RustCrypto |
| ed25519-dalek | 3.0.0 | plugin manifest signatures | BSD-3-Clause; `verify_strict` |
| getrandom | 0.4.3 | token generation | OS CSPRNG |
| jsonschema | 0.61.2 | contract validation | default features **off** → no remote `$ref` fetching (tested) |
| uuid (v7) / chrono / semver | 1.29 / 0.4.45 / 1.0.28 | ids, time, versions | |
| tracing / tracing-subscriber | 0.1 / 0.3.23 | structured logging | |
| clap | 4.6 | CLI | |
| thiserror / async-trait / libc | 2 / 0.1 / 0.2 | errors, async traits, rlimits | |

`cargo-deny` 0.20.2 (`deny.toml`) gates advisories, licences (permissive allow-list; `MIT-0` added for `borrow-or-share`, a transitive dependency of jsonschema), bans and sources.

**Exception pending approval:** RUSTSEC-2026-0459 / CVE-2026-119375 (bundled SQLite < 3.54 may return uninitialised page-cache memory for a crafted database + `-wal` pair). `sqlx-sqlite 0.9.0` caps `libsqlite3-sys < 0.38`; the fix ships in 0.39. The advisory is ignored in `deny.toml` with a reason; mitigations: EAEH-created data directory `0700`, database file `0600` (local attack vector requires write access to EAEH's data). Tracked as A-10.

## Consequences
Supply-chain checks run in `scripts/ci.sh`. The SQLite advisory exception must be explicitly accepted by the product owner; otherwise the phase exit is NOT READY.

## Alternatives considered
rusqlite (sync API; same SQLite advisory applies), Cedar (ADR-0006), OpenTelemetry SDK (optional, ADR-0010).
