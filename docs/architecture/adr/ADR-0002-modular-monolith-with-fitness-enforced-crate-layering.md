# ADR-0002: Modular monolith with fitness-enforced crate layering

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
The master instructions require modular boundaries, no premature microservices, no circular dependencies, and separation of governance from intelligent orchestration (clarification 5).

## Decision
A single Cargo workspace producing one binary (`eaeh`). Logical layers map to crates; the allowed normal-dependency set of every crate is declared in `tests/tests/fitness.rs` and verified from `cargo metadata` on every test run:

```
core ← store ← events ← governance ← extensibility ← {workspace, intelligence, tools}
core ← {isolation, validation}
{tools, intelligence, validation, workspace} ← agents
{governance, extensibility, workspace} ← workflow      (never agents / intelligence / tools)
everything ← runtime (composition root) ← api ← cli
{core, intelligence} ← testkit (dev-dependency only)
```

Additional fitness rules: no cycles; governance never depends on intelligence/agents/extensibility/workflow/tools/runtime; the workflow engine never depends on intelligence/agents/tools/runtime; `eaeh-testkit` is never a normal dependency; only `eaeh-store` depends on `sqlx`; `unsafe` only in `eaeh-isolation`; every production database write goes through the serialized writer; no literal tokens in source/config; every agent prompt is a versioned asset in `prompts/`.

## Consequences
Boundaries are compile-time enforced and regression-tested. Adding a crate requires adding a layering rule (the fitness test fails otherwise). Distribution can later follow crate boundaries.

## Alternatives considered
Microservices (rejected: operational complexity without a scale requirement); a single crate with modules (rejected: boundaries not enforceable).
