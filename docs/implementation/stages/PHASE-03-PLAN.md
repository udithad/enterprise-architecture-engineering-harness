# PHASE-03 – Plan: Requirements Engineering and Architecture Generation

| Field | Value |
|---|---|
| Phase | 3 – Requirements Engineering and Architecture Generation (master §30) |
| Date | 2026-11-12 |
| Authorization | "execute phase 3" (2026-11-12) |
| Baseline | `1527c09` + uncommitted Phase 1+2 work; 181/181 tests (`evidence/phase-02-ci.log`) |
| Phase 2 exit | **Recorded 2026-11-12: READY WITH ACCEPTED EXCEPTIONS (E-1…E-4)** (P3-D6) |
| Status | **APPROVED 2026-11-12 and implemented** — P3-D2…P3-D6 as recommended; P3-D1 live run authorised but **not executed** (no key supplied; product owner chose to skip). Deviation: BRD source kind `brd` replaced by `reference` + tag `brd` (ADR-0018, A-39). See `PHASE-03-COMPLETION.md` |

## 1. Inspection findings: reuse and gaps

| Existing asset | Reuse in Phase 3 | Gap / extension |
|---|---|---|
| Workflow engine: agent tasks get `workflow_input`, all prior `outputs` and `revision_feedback` (approver comments on rejection); `requires_approval_of`; gates bound to artifact hash | BRD → requirements → HLD → LLD chain with gates; revisions reuse the feedback loop | **No bound on revision loops** (only budgets); only `approve`/`reject` (no distinction between "revise" and "reject outright"); a state stores **one** artifact; `tool_step` input is static (cannot consume prior outputs); quality gates are **not run** by workflows |
| `eaeh-validation` (`Validator`, `QualityGate`, fail-closed, semantic never sole blocker) | Deterministic requirement/HLD/LLD validators and gates | Wire gates into workflow states and the agent correction loop |
| Agent runtime: output schema + correction loop with platform feedback, skills, RAG, citation validator, traceability | BA, requirements engineer, solution architect, designer agents | Run deterministic domain validators inside the correction loop (feedback = findings) |
| Knowledge (Phase 2): project-scoped documents, citations, mandatory standards in context | **BRD ingestion = governed knowledge document** (new source kind `brd`, project scope, default `confidential`); requirements cite BRD chunks; HLD/LLD cite standards | Source kind `brd`; ADR/HLD/LLD outputs re-ingestible as knowledge (later phases) |
| Skills (Phase 2) | Approved skills: `requirements-analysis`, `hld-design`, `lld-design` | Skill definitions + prompts |
| Artifact store (versioned, hash, provenance incl. skills/knowledge) | Requirements, HLD, LLD, ADRs, diagrams as versioned artifacts | Multi-artifact outputs per state |
| Model provider (Phase 2, mock-verified only) | Generation | **Real generation quality cannot be verified without a live model** (P3-D1) |

## 2. Design principle for this phase

Intelligent components **draft**; deterministic components **structure, check, render and
gate**; humans **approve**. Every generated document is a typed JSON model (validated by
schema and domain validators) from which Markdown and diagrams are **rendered
deterministically** — so diagrams and documents can never disagree with the approved model,
and traceability is computed, not claimed by the model.

## 3. Target design

### 3.1 New crates (deterministic; no dependency on agents/intelligence — fitness-enforced)

| Crate | Responsibility | Key types | Internal deps |
|---|---|---|---|
| `eaeh-requirements` | Requirements model; BRD section model; deterministic analysis: ambiguity (configurable weak-phrase lexicon, TBD/TBC markers, unmeasurable NFRs), conflicts (contradictory modality on the same subject; duplicate statements; conflicting numeric bounds), missing requirements (BRD sections without derived requirements; requirements without acceptance criteria; configurable NFR-category checklist), id/reference integrity; traceability (BRD chunk → requirement); renderer (Markdown) | `RequirementSet`, `Requirement` (`REQ-F-nnn`/`REQ-NF-nnn`, kind, NFR category, statement, rationale, priority MoSCoW, acceptance criteria, `sources` = BRD citations), `BusinessRule` (`BR-nnn`), `DomainConcept`, `Issue` (ambiguity/conflict/missing, severity, refs), validators | core, validation |
| `eaeh-architecture` | HLD and LLD models; validators (referential integrity, requirement coverage, LLD-refines-HLD, standards compliance, ADR completeness, risk/trade-off completeness); deterministic renderers: HLD/LLD Markdown, **Mermaid** diagrams (context/container, component, sequence, deployment, ER), ADR Markdown (MADR-style) | `Hld` (context, components, interfaces, data flows, deployment, decisions, risks, trade-offs, compliance, trace matrix), `Lld` (component designs, API operations, data entities, sequences, deployment nodes), `Decision`, `ComplianceEntry` (standard citation, status compliant/deviation/not-applicable, justification, ADR ref) | core, validation, requirements |
| `eaeh-engineering` | Governed tools wrapping the above (`requirements.analyze`, `architecture.validate`, `architecture.render`) for the tool gateway; validator registration; workflow bindings | tools | core, tools, validation, requirements, architecture |

### 3.2 Engine / runtime extensions (generic, not Phase-3-specific)

1. **Bounded revisions & distinct decisions**: approval decision `request_changes` (→ trigger
   `changes_requested`, comments become `revision_feedback`) vs `reject` (→ `rejected`,
   normally terminal); per-gate `max_revisions` (exceeded ⇒ escalate/fail). ADR required
   (changes the approval model).
2. **Quality gates on states**: `quality_gate: <id>` on work states; the engine runs the
   gate on the output before storing the artifact; blocking findings ⇒ state fails (or
   returns to the agent as feedback within `correction_attempts`). Gate results are stored
   with the artifact (provenance) and audited.
3. **Validators in the agent correction loop**: agent definitions may declare
   `output_validators`; findings are fed back as platform feedback (task-input trust).
4. **Tool-step input bindings**: `{"$output": "<state>"}` / `{"$input": "<pointer>"}`
   references resolved by the engine (deterministic, no expressions).
5. **Multi-artifact outputs**: a state may store several named artifacts (e.g. `hld.json`,
   `hld.md`, `diagrams/*.mmd`, `adr/ADR-*.md`); the approval gate binds to a **bundle
   manifest hash** covering all of them.
6. **Traceability store** (migration `0003`): `trace_links(from_kind, from_id, to_kind,
   to_id, artifact, artifact_version, hash)` written when an artifact is approved; API
   `GET /v1/traceability?project_id=&id=` (BRD chunk ↔ requirement ↔ HLD component ↔ LLD
   element ↔ ADR).

### 3.3 Agents, skills, prompts (configuration, versioned)

| Agent | Task kind | Output (schema) | Tools | Skills |
|---|---|---|---|---|
| `business-analyst` | `requirements-analysis` | `RequirementSet` draft (requirements, rules, concepts, citations to BRD chunks) | `knowledge.search` | `requirements-analysis@^1` |
| `requirements-engineer` | `requirements-review` | issues (ambiguity/conflict/missing) + refined set; deterministic analyzer findings merged | `knowledge.search`, `requirements.analyze` | `requirements-analysis@^1` |
| `solution-architect` | `hld-design` | `Hld` (incl. decisions → ADRs, risks, trade-offs, compliance with mandatory standards) | `knowledge.search`, `architecture.validate` | `hld-design@^1` |
| `software-designer` | `lld-design` | `Lld` (components, APIs, data, sequences, deployment) refining the approved HLD | `knowledge.search`, `architecture.validate` | `lld-design@^1` |

All read-only autonomy; no exec, no network, no file writes outside the artifact store.

### 3.4 Workflows

* `requirements.analysis`: analyse → deterministic analysis (tool step) → review (requirements
  engineer) → gate (`approver`; approve / request changes / reject).
* `architecture.hld`: design → validate (quality gate `hld`) → render (ADRs, diagrams, HLD.md)
  → gate. Requires approved requirements (input: approved requirements artifact).
* `architecture.lld`: design → validate (gate `lld`, incl. refinement of approved HLD) → render → gate.
* `engineering.design`: BRD → requirements gate → HLD gate → LLD gate in one durable instance
  (`requires_approval_of` enforces order independently of the graph).

### 3.5 Standards compliance

Compliance is checked against **approved mandatory standards actually supplied in context**
(Phase 2). Every such standard must have a compliance entry; `deviation` requires a
justification and an ADR; citations must have been retrieved. EAEH ships **no enterprise
standards** (rule 12); demo uses the labelled Phase 2 corpus plus a labelled demo BRD.

### 3.6 API / CLI

`eaeh brd ingest` (wraps knowledge ingest with source kind `brd`), `eaeh workflow start
engineering.design`, `eaeh approval decide … --decision request_changes`, `eaeh trace <id>`,
`GET /v1/traceability`, artifact bundle download. Existing approval and workflow routes are reused.

### 3.7 Demonstration

Labelled demo BRD (`examples/brd/order-management.md`) → requirements (with detected
ambiguity/missing items) → request-changes round trip → approved requirements → HLD with
ADRs, Mermaid diagrams, compliance table → approval → LLD with API/data/sequence/deployment
design → approval; traceability query BRD chunk → LLD element. Runs deterministically in CI
(scripted model double) and via `scripts/demo-design.sh` on a real server — **live model only
if authorised (P3-D1)**.

## 4. Implementation sequence (compile + test after each)

| Step | Deliverable |
|---|---|
| 0 | ADR-0018…0022 (requirements model & analysis; architecture models & deterministic rendering; approval decisions & bounded revisions; quality gates & bindings in workflows; traceability store) |
| 1 | `eaeh-requirements`: model, validators, analyzers, renderer + unit tests |
| 2 | `eaeh-architecture`: HLD/LLD models, validators, Mermaid/Markdown/ADR renderers + tests |
| 3 | Governance/engine: `request_changes`, `max_revisions`, quality gates on states, input bindings, multi-artifact bundles; migration 0003 traceability |
| 4 | Agent runtime: `output_validators` in correction loop |
| 5 | `eaeh-engineering` tools; runtime wiring; source kind `brd` |
| 6 | Agents, skills, prompts, workflows; API/CLI |
| 7 | Tests: unit, integration, security (cross-project BRD isolation, no self-approval of HLD, gate bypass, injected BRD instructions, revision bounds), e2e demo, fitness |
| 8 | Live verification (only if P3-D1 authorised) |
| 9 | Docs, ADRs, roadmap, action register, test strategy, `PHASE-03-COMPLETION.md` |

## 5. Acceptance criteria

| ID | Criterion |
|---|---|
| AC3-01 | Full gate passes; all Phase 1–2 tests remain green |
| AC3-02 | Fitness: new crates layered (no agents/intelligence deps in requirements/architecture) |
| AC3-03 | BRD ingested as project-scoped, classified, citable knowledge; never visible to other projects |
| AC3-04 | Requirements model: functional/NFR, business rules, domain concepts, acceptance criteria; schema + domain validation; unique ids |
| AC3-05 | Deterministic ambiguity, conflict and missing-requirement detection on fixed fixtures (precision/recall recorded) |
| AC3-06 | Traceability BRD chunk → requirement: every requirement cites retrieved BRD chunks; uncovered BRD sections reported |
| AC3-07 | HLD model + validators: referential integrity, every Must requirement allocated, decisions → ADRs, risks/trade-offs present |
| AC3-08 | Standards compliance: every mandatory standard in context has an entry; deviations need justification + ADR |
| AC3-09 | Deterministic rendering: HLD/LLD Markdown, Mermaid context/component/sequence/deployment/ER diagrams, ADRs; identical model ⇒ identical bytes; diagram text escaping prevents injection |
| AC3-10 | LLD validators: refines approved HLD (components exist), APIs/data/sequences/deployment referentially consistent; requirement coverage carried through |
| AC3-11 | Approval: HLD/LLD/requirements gates bound to bundle hash; SoD; agents cannot approve; gates cannot be bypassed via API |
| AC3-12 | Revision (`request_changes` → feedback → new version) and rejection (terminal) workflows; `max_revisions` enforced |
| AC3-13 | Quality gates on workflow states fail closed; validator findings drive bounded agent correction |
| AC3-14 | Traceability store and query across BRD ↔ requirement ↔ HLD ↔ LLD ↔ ADR, recorded only for approved artifacts |
| AC3-15 | Injected instructions in a BRD cannot alter agent instructions, tools or approvals |
| AC3-16 | Provenance: model, prompt, skill, knowledge (BRD + standards) versions recorded for every generated artifact |
| AC3-17 | API/CLI for BRD ingestion, design workflows, request-changes, traceability |
| AC3-18 | Demo end-to-end (CI, scripted model) |
| AC3-19 | Docs, ADRs, roadmap, action register, test strategy, completion report |
| AC3-20 | (Only if P3-D1 authorised) Live run: BRD → requirements → HLD → LLD with a real model, outputs passing deterministic gates, recorded with cost |

## 6. Risks

| ID | Risk | Mitigation |
|---|---|---|
| R3-01 | Without a live model, generation quality is unverified | P3-D1; deterministic gates; honest reporting |
| R3-02 | Lexicon-based ambiguity detection has false positives/negatives | Configurable lexicon; findings advisory unless severity blocking; measured on fixtures |
| R3-03 | Large typed outputs exceed model output limits | Per-section generation (HLD then LLD per component) if needed; schema size limits |
| R3-04 | BRDs contain confidential data → egress | Default `confidential`; egress policy; A-33 review before live use |
| R3-05 | Engine changes (decisions, bundles) affect Phase 1 approval semantics | Backward-compatible (`reject` unchanged); regression suite |
| R3-06 | E-1 SQLite advisory with more stored business content | Unchanged; upstream still blocked |

## 7. Decisions requested

| ID | Decision | Recommendation |
|---|---|---|
| P3-D1 | Live model verification | Authorise a bounded live run (your key via `EAEH_SECRET_ANTHROPIC_API_KEY`, cost cap) — otherwise generation is reported IMPLEMENTED BUT NOT VERIFIED with a real model |
| P3-D2 | BRD input formats | Markdown and plain text now; DOCX/PDF later (A-35) |
| P3-D3 | Diagram format | Mermaid text rendered deterministically from the model (no image rendering infrastructure) |
| P3-D4 | Approval semantics | Add `request_changes` distinct from `reject`; `max_revisions` per gate (default 3) |
| P3-D5 | NFR completeness checklist | ISO/IEC 25010 quality characteristics as a configurable default checklist (not an enterprise standard) |
| P3-D6 | Phase 2 exit | Record Phase 2 exit as READY WITH ACCEPTED EXCEPTIONS (E-1…E-4) |
