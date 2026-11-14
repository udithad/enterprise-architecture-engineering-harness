# EAEH Implementation Roadmap

Source of scope: `claude-instructions.md` Part 10. Implementation completion, phase
acceptance and authorization to proceed are separate events.

| Phase | Title | Status | Notes |
|---|---|---|---|
| 1 | Core Harness Foundation | **Implementation complete — awaiting exit decision** | Report: `stages/PHASE-01-COMPLETION.md`; proposed exit: READY WITH ACCEPTED EXCEPTIONS (pending A-10) |
| 2 | Enterprise Knowledge and Skills Management | **Accepted — READY WITH ACCEPTED EXCEPTIONS (E-1…E-4), 2026-11-12** | Plan `stages/PHASE-02-PLAN.md`; report `stages/PHASE-02-COMPLETION.md` |
| 3 | Requirements Engineering and Architecture Generation | **Implementation complete — awaiting exit decision** | Plan `stages/PHASE-03-PLAN.md` (approved); report `stages/PHASE-03-COMPLETION.md`; proposed exit: READY WITH ACCEPTED EXCEPTIONS (live model run not executed) |
| 4 | Multi-Agent Software Development | Not started — requires explicit authorization | Recommendations below |
| 5 | Testing and Quality Engineering | Not started | Includes agent evaluation and performance testing |
| 6 | Git and CI/CD Integration | Not started | |
| 7 | Release and Deployment Management | Not started | |
| 8 | Enterprise Scale and Advanced Capabilities | Not started | Enhances (does not introduce) security, isolation, audit |

## Phase 1 delivered (summary)

16 workspace crates, one binary (`eaeh`), 140 automated tests (all passing, including 7
real container-escape tests), fitness-enforced layering, governed tool gateway, durable
workflow engine with approval gates and recovery, dynamic agents with bounded delegation,
isolation providers with declared capabilities, deterministic policy engine, hash-chained
audit, transactional outbox events, versioned prompts, trust-aware context assembly,
provider-independent model management (no production provider by decision D-04), `/v1` API
and CLI. Details and exceptions: `stages/PHASE-01-COMPLETION.md`.

## Phase 2 delivered (summary)

Three new crates (`eaeh-knowledge`, `eaeh-skills`, `eaeh-provider-anthropic`; 19 crates in
total), migration `0002`, governed knowledge sources and versioned documents (Markdown, text,
JSON, OpenAPI-JSON) with quarantine, hash-bound approval of standards, lifecycle and purge;
FTS5 + hashed-vector hybrid retrieval with scopes, classification ceilings, freshness,
conflicts and citations; skills registry with digest-bound approval and permission-preserving
deterministic binding; RAG integration in the agent runtime with mandatory standards,
citation validation and full traceability; `model.invoke` egress policy; opt-in Anthropic
adapter (mock-verified only); `/v1` routes and CLI; demo corpus, skill, workflow and script.
181 automated tests pass (140 Phase 1 + 41 Phase 2). Details, evidence and exceptions:
`stages/PHASE-02-COMPLETION.md`.

## Phase 3 delivered (summary)

New crates:
- `eaeh-requirements` (pure): requirements model, deterministic analysis, BRD coverage, traceability, rendering.
- `eaeh-architecture` (pure): HLD/LLD models, validators, and deterministic Markdown, Mermaid and ADR rendering.
- `eaeh-engineering`: governed design tools.

This brings the total to 22 crates. Engine and governance gained:
- `request_changes` with bounded revisions;
- quality gates on workflow states and validators in the agent correction loop;
- input bindings and multi-file bundles bound to one approval;
- a traceability store fed only by approved artifacts;
- whole-document knowledge reads (`knowledge.read`, `brd.sections`).

Also delivered: migration 0003; agents (business analyst, requirements engineer, solution architect, software designer), skills, prompts, and the workflows `requirements.analysis` and `engineering.design`; API and CLI (`brd ingest`, `trace`, `artifact export`, `/v1/traceability`); a demonstration BRD, design fixtures and `scripts/demo-design.sh`.

214 automated tests pass. **No real-model run was executed:** P3-D1 was authorised, but no key was supplied. Details: `stages/PHASE-03-COMPLETION.md`.

## Phase 4 – recommendations (for authorization; not started)

Scope (master §31): multi-agent software development from approved LLDs.

Recommended entry criteria:
* Phase 3 exit decision recorded (A-38).
* **A-30 live run executed and reviewed.** Phase 4 consumes Phase 3 designs, whose quality with a real model is currently unverified.
* A-09 real standards and reference architectures; A-33 egress review.
* A-08/A-26 CI on Linux with container isolation (Phase 4 executes generated code).

Recommended early Phase 4 work:
- A-40: streaming and per-section generation.
- A-21: admin routes for agents and workflows.
- A-42: version-aware traceability extended to code artifacts.
- A-37: evaluation sets.
