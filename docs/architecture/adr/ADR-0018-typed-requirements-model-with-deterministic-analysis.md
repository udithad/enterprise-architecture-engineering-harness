# ADR-0018: Typed requirements model with deterministic analysis

| Field | Value |
|---|---|
| Status | Accepted (P3-D2, P3-D5) |
| Date | 2026-11-12 |
| Phase | 3 – Requirements Engineering and Architecture Generation |
| Deciders | Product owner (approval of PHASE-03-PLAN), Principal architect |

## Context
Master §30 requires BRD ingestion and analysis, requirements decomposition (functional, non-functional, business rules, domain concepts), ambiguity/conflict/missing-requirement detection and traceability. Language models can draft requirements but cannot be the authority on structure, coverage or traceability.

## Decision
* New crate `eaeh-requirements` (depends only on `eaeh-core` and `eaeh-validation`; no storage, agents, models, tools or network — fitness test `design_models_are_deterministic_and_independent_of_intelligence`).
* Typed, strictly parsed (`deny_unknown_fields`) model: `RequirementSet` with `Requirement` (`REQ-F-nnn`/`REQ-NF-nnn`, kind, NFR quality category, statement, rationale, MoSCoW priority, acceptance criteria, BRD source citations, business-rule and concept references), `BusinessRule` (`BR-nnn`), `DomainConcept`, `Issue` (ambiguity/conflict/missing/duplicate; origin agent or analyzer), `ScopeExclusion`.
* Deterministic validator `requirements.set`: ids, references, citation format, every source listed in `citations` (so the agent runtime's citation validator verifies retrieval), Must ⇒ acceptance criteria; analysis: vague-phrase lexicon and unresolved markers (TBD/TBC), unmeasurable NFRs, negation-contradiction, numeric-target conflicts and duplicates (token similarity with configurable thresholds), missing NFR categories (configurable checklist, ISO/IEC 25010 characteristics by default), and **BRD coverage** — every BRD section must be covered or explicitly excluded with a reason. Ambiguities the analyst has already raised as issues are downgraded to information.
* Severity policy: structural defects, unacknowledged TBDs, contradictions and uncovered BRD sections block (`error`); vague wording, numeric conflicts, duplicates and missing categories are warnings for human review.
* BRDs are governed knowledge documents (Phase 2): project-scoped, default classification `confidential`, tag `brd`, read in full through the new `knowledge.read` tool and outlined by the `brd.sections` tool (both scope- and classification-filtered and audited). Formats: Markdown and plain text (P3-D2).
* **Deviation from the plan:** the plan proposed a new knowledge source kind `brd`. SQLite cannot widen the `knowledge_sources.kind` CHECK constraint without rebuilding four dependent tables, so BRDs use source kind `reference` plus the `brd` tag. Recorded as A-39.

## Consequences
Requirement quality checks are reproducible and explainable; the analyzer flags candidates, it does not understand text — measured baseline on a labelled 24/8-item fixture set: ambiguity precision 0.923, recall 0.857, pair accuracy 0.875 (labels written by the implementer, A-37). Semantic contradictions are not detected.

## Alternatives considered
LLM-only analysis (rejected: not reproducible, cannot be sole basis for gates); NLP libraries (rejected for now: new dependencies with limited benefit on short statements).
