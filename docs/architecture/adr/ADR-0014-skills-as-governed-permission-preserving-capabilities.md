# ADR-0014: Skills as governed, permission-preserving capabilities

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-12 |
| Phase | 2 – Enterprise Knowledge and Skills Management |
| Deciders | Product owner (approval of PHASE-02-PLAN), Principal architect |

## Context
Master §18: skills are reusable capabilities (not agent prompt text) with registration, versioning, dependencies, contracts, required knowledge/tools, permissions, validation and retirement; skill versions must be recorded.

## Decision
* New crate `eaeh-skills`. Definition: id, semver version, description, capabilities, task kinds, instructions (plain text, no placeholders), required knowledge queries, required tools, input/output JSON Schemas, `max_risk`, dependencies (`id` + semver req), platform compatibility (`eaeh_api`), evaluation criteria.
* Registration validates contracts, tool existence and risk ≤ `max_risk`, dependency resolvability and acyclicity; versions are immutable; every version requires **human approval bound to the definition digest**; `approved` and `deprecated` are usable, `retired`/`rejected`/`pending` are not.
* **Binding rule — a skill never grants permissions**: it applies to an agent only if all required tools are already in the agent's allow-list and `max_risk` ≤ the agent's autonomy ceiling. Explicit incompatible references fail the run; dynamically matched incompatible skills are skipped and reported.
* Selection: explicit refs (`id` or `id@req`) + approved skills matching the task kind; highest usable version (approved preferred); dependencies placed first; deterministic.
* Applied skills contribute instructions at `skill` trust (below agent instructions and mandatory standards), their required-knowledge queries to retrieval, and their input/output contracts to validation. Skill versions are recorded in agent results, `model_usage.skill_refs` and artifact provenance.
* Only `eaeh-skills` may construct skill-instruction context items (fitness test).

## Consequences
Skills are reusable across agents and fully traceable. Agent definitions' `skills` field changed from opaque strings to parsed references (backwards-compatible JSON).
