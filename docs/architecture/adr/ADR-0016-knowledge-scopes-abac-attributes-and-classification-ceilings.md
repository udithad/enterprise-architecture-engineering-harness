# ADR-0016: Knowledge scopes, ABAC attributes and classification ceilings

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-12 |
| Phase | 2 – Enterprise Knowledge and Skills Management |
| Deciders | Product owner (approval of PHASE-02-PLAN), Principal architect |

## Context
Master §17 requires enterprise, domain, project, team and workflow scopes and prevention of cross-project/tenant leakage.

## Decision
* Document scope: `enterprise | domain:<name> | project:<id> | team:<name> | workflow:<id>`; tenant always enforced.
* ABAC attributes: `projects.domain` and `principals.teams` (admin-managed, audited).
* Visibility for a reader = enterprise ∪ domain(of the project) ∪ project ∪ team(reader's teams) ∪ workflow(current instance), filtered by classification ceiling.
* Ceilings: Admin/KnowledgeSteward `restricted`; other humans `knowledge.human_max_classification` (default `confidential`); agents `knowledge.agent_max_classification` (default `confidential`).
* Agents are project-bound, have no teams (team-scoped knowledge is never visible to agents in Phase 2), and see workflow-scoped knowledge only for the workflow instance of their task. The `knowledge.search` tool resolves context from the calling principal; it cannot widen scope.

## Consequences
Fail-closed visibility; team knowledge for agents requires a later decision (A-31).
