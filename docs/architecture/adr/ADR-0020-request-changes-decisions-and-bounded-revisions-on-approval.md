# ADR-0020: Request-changes decisions and bounded revisions on approval gates

| Field | Value |
|---|---|
| Status | Accepted (P3-D4) |
| Date | 2026-11-12 |
| Phase | 3 – Requirements Engineering and Architecture Generation |
| Deciders | Product owner (approval of PHASE-03-PLAN), Principal architect |

## Context
Master §30 requires revision and rejection workflows. Phase 1 had only approve/reject, and a rejected gate looped back without bound (only budgets limited it).

## Decision
* New decision `request_changes` (status `changes_requested`, event `approval.changes_requested`), allowed **only for workflow gates** and **only with comments**; knowledge-standard and skill approvals stay approve/reject.
* Engine trigger `changes_requested`; comments become `revision_feedback` for the next run of the producing state; the previous output stays available (`input.outputs`) and the new output is a new artifact version.
* Per-gate `max_revisions` (default 3, max 20): a further change request follows the `rejected` transition with a recorded reason; every revision is audited (`workflow.revision`).
* Backward compatible: gates without a `changes_requested` transition use their `rejected` transition (Phase 1/2 workflows unchanged).
* Migration `0003` widens the CHECK constraints of `approvals` / `approval_decisions` by rebuilding both tables inside the migration transaction (child rebuilt against the new parent first, so foreign keys stay enforced).

## Consequences
Reviewers can distinguish 'revise' from 'stop'; revision loops are bounded and auditable.

## Alternatives considered
Treat every rejection as a revision (rejected: no final rejection); unbounded loops (rejected).
