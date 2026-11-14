# ADR-0017: knowledge_steward role and approval separation of duties

| Field | Value |
|---|---|
| Status | Accepted (P2-D4) |
| Date | 2026-11-12 |
| Phase | 2 – Enterprise Knowledge and Skills Management |
| Deciders | Product owner (approval of PHASE-02-PLAN), Principal architect |

## Context
Knowledge curation must be separable from approval of standards and skills.

## Decision
* New role `knowledge_steward`: `knowledge.manage`, `knowledge.read`, `skill.manage`, `skill.read`, `approval.read`, `token.manage_own`; **no** `approval.decide`.
* Admin gains knowledge/skill management; Operator, Approver and Viewer gain `knowledge.read`/`skill.read`; agents gain only `knowledge.read` (via the governed tool and context assembly).
* Standards and skills create approval requests (subject types `knowledge_document`, `skill`) bound to the content hash / definition digest; the registrant is recorded as author and can never approve. The harness, API and CLI apply decided approvals to the subject (`apply_approval`).

## Consequences
Curators cannot self-publish mandatory standards or skills.
