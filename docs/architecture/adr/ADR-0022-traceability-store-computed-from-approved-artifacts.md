# ADR-0022: Traceability store computed from approved artifacts

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-12 |
| Phase | 3 – Requirements Engineering and Architecture Generation |
| Deciders | Product owner (approval of PHASE-03-PLAN), Principal architect |

## Context
Master §30 and Part 14 require traceability from business requirements to design (and later code). Links asserted by a model are not trustworthy, and links from unapproved drafts are noise.

## Decision
* Rendering tools emit `trace` links computed from validated models (`brd_section → requirement`, `requirement → hld_component | hld_interface | decision`, `decision → standard`, `hld_component → lld_component`, `requirement → api_operation | sequence | lld_component`, `lld_component → entity`).
* The engine records them in `trace_links` (migration 0003) **only when the gate approving that artifact is approved**, keyed by tenant, project and artifact, with artifact name/version/hash, workflow instance and approval id; idempotent; audited (`trace.record`) and evented (`trace.links_recorded`).
* Query: `WorkflowEngine::trace` (authorized `workflow.read` on the project) with optional item and transitive closure depth 1–10 in both directions; `GET /v1/traceability`, `eaeh trace`.

## Consequences
Every link is attributable to an approved artifact version and a human approval. Superseded versions keep their links (history); queries currently return all versions (A-42).

## Alternatives considered
Graph database (rejected: infrastructure); storing links in artifacts only (rejected: not queryable).
