# PHASE-03 – Completion Report: Requirements Engineering and Architecture Generation

| Field | Value |
|---|---|
| Phase | 3 – Requirements Engineering and Architecture Generation (master §30) |
| Date | 2026-11-12 |
| Plan | `PHASE-03-PLAN.md`, approved 2026-11-12. P3-D2…P3-D6 as recommended. P3-D1 (live run) authorised. |
| Baseline | `1527c09` + uncommitted Phase 1–2 work (181/181 tests); Phase 2 accepted with exceptions E-1…E-4 |
| Result | Implementation complete; **214/214 automated tests pass**; quality gate green |
| Live model run | **Not executed.** It was authorised (P3-D1), but no API key was supplied, and the product owner chose to skip it. |
| Proposed exit decision | **READY WITH ACCEPTED EXCEPTIONS.** Requires your acceptance of E-5 and E-6 (§14). |
| Version control | **Nothing committed or pushed** (A-27) |
| Phase 4 | **Not started** |

## 1. Executive summary

Phase 3 turns the harness into a governed requirements-and-architecture pipeline:

- A **BRD** is ingested as project-scoped, confidential, citable knowledge.
- A **business analyst** agent and a **requirements engineer** agent draft and refine a typed requirements document: functional and non-functional requirements, business rules, domain concepts and issues.
- A **solution architect** agent and a **software designer** agent produce typed **HLD** and **LLD** models.

The platform applies deterministic controls to every agent step:
- It validates the output, both inside the agent's correction loop and as a fail-closed quality gate before storage.
- It analyses the requirements for ambiguity, conflicts, duplicates, gaps and BRD coverage.
- It renders Markdown documents, ADRs and Mermaid diagrams from the validated model.
- It checks compliance with every mandatory enterprise standard.
- It requires a human approval bound to the bundle of all rendered files. The approver can **approve, request changes** (bounded revisions with feedback to the agent) **or reject**.
- It records traceability only for approved artifacts, from BRD section to requirement to HLD component or decision to LLD API operation.

All of this is verified with production code end to end. The only exception is the model, which is the labelled scripted test double. **No real model has generated requirements or designs yet**, so the *content quality* of Phase 3 output is unverified (E-5).

## 2. Expected versus actual deliverables (master §30)

| Master §30 item | Delivered | Where | Verification |
|---|---|---|---|
| BRD ingestion and analysis | Yes (Markdown/text) | `eaeh brd ingest`, `knowledge.read`, `brd.sections`, business-analyst agent | `design_demo.rs`, `cli.rs::phase3_…` |
| Requirements decomposition | Yes | `RequirementSet` (`eaeh-requirements`) | `requirements.rs` |
| Functional and non-functional requirements | Yes (NFR quality categories) | same | same |
| Business rules and domain concepts | Yes | same | same |
| Ambiguity, conflict and missing-requirement detection | Yes – deterministic heuristics + analyst issues | `analysis.rs` | `requirements.rs`, `analyzer_eval.rs` (baseline) |
| Requirements traceability | Yes | BRD coverage, `trace_links`, `/v1/traceability` | `design_demo.rs`, `design_flow.rs` |
| Business Analyst / Requirements Engineer capabilities | Yes (agents, prompts, approved skill) | `config/agents/business-analyst.json`, `requirements-engineer.json`, `examples/skills/requirements-analysis.json` | scripted model only |
| Solution Architect capabilities | Yes | `solution-architect.json`, `hld-design` skill | scripted model only |
| HLD generation | Yes | `engineering.design` (`hld`, `render_hld`) | `design_demo.rs` (scripted) |
| Architecture diagrams | Yes – Mermaid, generated deterministically | `mermaid.rs` | `architecture.rs` |
| ADR generation | Yes – one per decision | `render.rs::adr` | `architecture.rs`, `design_demo.rs` |
| Standards compliance | Yes – every supplied mandatory standard must be addressed | `validate.rs::check_compliance` | `architecture.rs`, `design_flow.rs` |
| Risk and trade-off analysis | Yes – required risks, trade-offs, decision alternatives | HLD model + validator | `architecture.rs` |
| HLD validation and human approval | Yes | gate `hld` + `approve_hld` | `design_demo.rs` |
| LLD generation | Yes | software-designer agent, `lld` state | `design_demo.rs` (scripted) |
| Component, API and data design | Yes | `Lld` | `architecture.rs` |
| Sequence and deployment diagrams | Yes | `mermaid::{sequence, deployment, er, modules}` | `architecture.rs`, `design_demo.rs` |
| LLD validation and human approval | Yes | gate `lld` + `approve_lld` | `design_demo.rs` |
| Revision and rejection workflows | Yes | `request_changes`, `max_revisions`, `reject` | `design_flow.rs`, `api.rs`, `cli.rs` |
| "BRD analysis and HLD/LLD generation become usable" | **Pipeline usable; real-model quality unverified** | – | live run not executed (E-5) |

## 3. Architecture and implementation summary

ADR-0018…0022. The architecture document is `docs/architecture/engineering-design.md`.

- **New crates (22 crates in total):**
  - `eaeh-requirements` and `eaeh-architecture` are *pure*: core and validation only, with no storage, agents, models, tools or network. A fitness test enforces this.
  - `eaeh-engineering` provides governed read-only tools and is used only by the composition root.
- **Engine (generic):**
  - `changes_requested` trigger and `max_revisions`;
  - deterministic bindings (`$output`, `$input`, `$artifact`);
  - quality gates on work states (fail closed, configured by the platform);
  - multi-file bundles bound by a manifest;
  - traceability recorded when a gate is approved.
- **Governance:** `request_changes` decision, allowed only for workflow gates and only with comments.
- **Agents:**
  - `output_validators` run inside the correction loop.
  - The registered output schema is shown to the model.
  - The mandatory standards supplied to a run are recorded in results and artifact provenance.
- **Knowledge:** `read_document` and `knowledge.read` apply the same visibility predicate as search.
- **Storage:** migration `0003` rebuilds `approvals` and `approval_decisions` to widen their CHECK constraints, and adds `trace_links`.
- **Dependencies:** no new third-party dependencies. `cargo deny` reports advisories, bans, licences and sources ok; the E-1 ignore is unchanged.

## 4. Component responsibilities and data flows

See the component table and workflow diagram in `docs/architecture/engineering-design.md`. Data flow per agent stage:

```
workflow input / approved earlier outputs ──bindings──► agent task input (data)
mandatory standards (enterprise_policy) + approved skill (skill) ──► instructions
knowledge.read / knowledge.search ──► cited, fenced, untrusted data
model draft ──► schema + skill contract + citation validator + domain validator ─(findings)─► correction
          └──► engine quality gate (fail closed) ──► artifact (provenance: model, prompts, skill, knowledge, standards, gates)
render tool ──► bundle (files + manifest hash + computed trace) ──► human gate (approve | request_changes | reject)
approve ──► trace_links (bound to artifact version, hash, approval)
```

## 5. Implementation inventory

| Area | Files |
|---|---|
| Requirements | `crates/eaeh-requirements/src/{lib,model,analysis,render}.rs` (1,271 lines); tests `tests/{requirements,analyzer_eval}.rs` (462) |
| Architecture | `crates/eaeh-architecture/src/{lib,model,validate,mermaid,render}.rs` (1,997); tests `tests/architecture.rs` (296) |
| Engineering tools | `crates/eaeh-engineering/src/lib.rs` (305); tests `tests/tools.rs` |
| Engine | `crates/eaeh-workflow/src/{definition,engine}.rs` (bindings, gates, bundles, revisions, trace); tests `tests/design_flow.rs` |
| Governance / events | `crates/eaeh-governance/src/approval.rs`, `crates/eaeh-events/src/catalog.rs` (`approval.changes_requested`, `trace.links_recorded`) |
| Agents | `crates/eaeh-agents/src/{definition,runtime}.rs` |
| Knowledge | `crates/eaeh-knowledge/src/{search,tool,lib}.rs` |
| Runtime / API / CLI | `crates/eaeh-runtime/src/{harness,config,executor}.rs`, `crates/eaeh-api/src/routes.rs` (`/v1/traceability`), `crates/eaeh-cli/src/main.rs` (`brd`, `trace`, `artifact export`, `request_changes`) |
| Storage | `crates/eaeh-store/migrations/0003_engineering_design.sql` |
| Configuration | `config/agents/{business-analyst,requirements-engineer,solution-architect,software-designer}.json`; `prompts/agent.{business-analyst,requirements-engineer,solution-architect,software-designer}.toml`; `workflows/{requirements.analysis,engineering.design}.json`; `examples/skills/{requirements-analysis,hld-design,lld-design}.json` |
| Demonstration | `examples/brd/order-management.md`, `examples/design/{requirements,hld,lld}.json` (labelled fixtures), `scripts/demo-design.sh`, `tests/tests/design_demo.rs` |
| Docs | ADR-0018…0022; `docs/architecture/engineering-design.md`; updates to architecture README, components, runtime, data, security, threat model (T41–T50), OPERATIONS, TEST-STRATEGY, ROADMAP, ACTION-ITEMS, README |

### Changes to Phase 1–2 code and tests (disclosed)

**Behaviour changes:**
- Agents now receive their registered output schema as a platform data item. This applies to all agents.
- Migration 0003 rebuilds the approval tables. Existing rows are copied, and the store, API, runtime and smoke tests pass on the migrated schema.
- A gate without a `changes_requested` transition treats `request_changes` as its `rejected` transition. Earlier workflows are otherwise unchanged.
- The workflow crate now depends on `eaeh-validation`, which is pure. The fitness layering was updated accordingly.

**Test updates:**
- Migration assertions now expect `[1, 2, 3]`.
- The runtime test's expected agent and workflow lists include the new definitions.
- The test-only agent definition literal gained `output_validators: vec![]`.
- No earlier test was removed or ignored. No assertion was weakened.

### Defects found and fixed during the phase

- **Upper-case bundle paths:** paths such as `adr/D-001-…` violated the platform's lower-case artifact-name rule, so rendering failed in the end-to-end test. The renderer now lower-cases paths, and `rendering_is_deterministic_and_complete` asserts that every bundle path is a valid artifact name.
- **Wrong test expectations:** several new tests had wrong expectations (an entity-code `;`, a requirement still allocated through a decision, an operation that did belong to the step's target). These were corrected after checking that the code was right; the reasoning is recorded in the test comments.

## 6. Acceptance criteria and statuses

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| AC3-01 | Full gate; Phase 1–2 tests green | IMPLEMENTED AND VERIFIED | `phase-03-ci.log`: 214 passed, 0 failed, 0 ignored |
| AC3-02 | New crates layered; design crates pure | IMPLEMENTED AND VERIFIED | `fitness.rs` (11), incl. `design_models_are_deterministic_and_independent_of_intelligence` |
| AC3-03 | BRD as project-scoped, classified, citable knowledge; no cross-project visibility | IMPLEMENTED AND VERIFIED | `design_demo.rs::brds_are_project_scoped_and_classification_bounded`, `cli.rs::phase3_…`; `phase-03-design-demo.log` |
| AC3-04 | Requirements model, schema + domain validation, unique ids | IMPLEMENTED AND VERIFIED | `requirements.rs` |
| AC3-05 | Ambiguity / conflict / missing detection; precision/recall recorded | IMPLEMENTED AND VERIFIED (heuristic; implementer-labelled baseline) | `requirements.rs`, `analyzer_eval.rs`; `phase-03-analyzer-eval.log` |
| AC3-06 | BRD → requirement traceability; uncovered sections reported | IMPLEMENTED AND VERIFIED | `requirements.rs::uncovered_brd_sections_are_missing_requirements`, `design_demo.rs` |
| AC3-07 | HLD validation (integrity, Must allocation, decisions → ADRs, risks/trade-offs) | IMPLEMENTED AND VERIFIED | `architecture.rs` |
| AC3-08 | Standards compliance (every mandatory standard; deviation ⇒ justification + ADR) | IMPLEMENTED AND VERIFIED | `architecture.rs::mandatory_standards_…`, `design_flow.rs::quality_gate_…` |
| AC3-09 | Deterministic rendering; injection-safe diagrams | IMPLEMENTED AND VERIFIED | `architecture.rs::rendering_is_deterministic_and_complete`, `::model_text_cannot_inject_…`, `mermaid::tests::…` |
| AC3-10 | LLD validation, refinement of approved HLD, coverage carried through | IMPLEMENTED AND VERIFIED | `architecture.rs::lld_must_refine_…` |
| AC3-11 | Gates bound to bundle hash; SoD; agents cannot approve; no API bypass | IMPLEMENTED AND VERIFIED | `design_flow.rs::bundles_bind_…`, `api.rs::request_changes_…`, Phase 1 gate tests |
| AC3-12 | Revision and rejection workflows; `max_revisions` | IMPLEMENTED AND VERIFIED | `design_flow.rs` (2), `approval::tests::request_changes_…`, `api.rs`, `cli.rs` |
| AC3-13 | Quality gates fail closed; validator-driven correction | IMPLEMENTED AND VERIFIED | `design_flow.rs::quality_gate_fails_closed_…`, `design_demo.rs` (BA corrected after `[requirements.set]` feedback) |
| AC3-14 | Traceability store/query; only approved artifacts | IMPLEMENTED AND VERIFIED | `design_flow.rs::bundles_bind_…`, `design_demo.rs` |
| AC3-15 | BRD injection cannot alter instructions, tools or approvals | IMPLEMENTED AND VERIFIED | `design_demo.rs::injected_brd_instructions_…` |
| AC3-16 | Provenance of model, prompt, skill, knowledge and standards versions | IMPLEMENTED AND VERIFIED | `design_demo.rs` (artifact provenance, `model_usage` refs) |
| AC3-17 | API/CLI for BRD, design workflows, request changes, traceability | IMPLEMENTED AND VERIFIED | `api.rs`, `cli.rs::phase3_…`, `tools.rs`; `phase-03-design-demo.log` |
| AC3-18 | Demo end to end (CI, scripted model) | IMPLEMENTED AND VERIFIED (scripted model) | `design_demo.rs::brd_to_requirements_hld_and_lld_with_gates_revisions_and_traceability` |
| AC3-19 | Docs, ADRs, roadmap, action register, test strategy, report | IMPLEMENTED AND VERIFIED | this report; ADR-0018…0022; updated documents |
| AC3-20 | Live run BRD → requirements → HLD → LLD with a real model | **NOT IMPLEMENTED** (authorised; not executed – no key supplied) | – (A-30) |

**Real-model generation of requirements, HLD and LLD: IMPLEMENTED BUT NOT VERIFIED.** The agents, prompts, skills, the Anthropic adapter (mock-verified in Phase 2) and the output-schema exposure are in place. No real model has produced Phase 3 output, so it is unknown whether a real model passes the deterministic gates within the correction budget.

## 7. Testing instructions (exact commands, expected outputs)

```bash
scripts/ci.sh
#   expected last line: "==> CI PASSED"; 214 tests "ok"
cargo test -p eaeh-requirements
#   10 tests ok
cargo test -p eaeh-requirements --test analyzer_eval -- --nocapture
#   "ANALYZER-EVAL ambiguity n=24 … precision=0.923 recall=0.857"
#   "ANALYZER-EVAL pairs n=8 correct=7 accuracy=0.875"
cargo test -p eaeh-architecture
#   9 tests ok
cargo test -p eaeh-engineering
#   2 tests ok
cargo test -p eaeh-workflow --test design_flow
#   5 tests ok
cargo test -p eaeh-acceptance --test design_demo
#   3 tests ok
scripts/demo-design.sh
#   expected last line: "== DEMO COMPLETE (live model: false)"
#   without a provider the workflow ends "failed – analyse: unavailable: no registered model …"
EAEH_DEMO_LIVE=1 EAEH_ANTHROPIC_KEY_FILE=~/.eaeh/anthropic.key scripts/demo-design.sh
#   live; billed; not yet run
```

## 8. Actual test results and evidence

| Evidence | Produced by | Result |
|---|---|---|
| `evidence/phase-03-ci.log` (2026-11-12T18:17:10Z) | `scripts/ci.sh`: fmt, clippy `-D warnings`, build, all tests, cargo-deny | **214 passed, 0 failed, 0 ignored** (47 test binaries); `==> CI PASSED` |
| `evidence/phase-03-analyzer-eval.log` (18:00:43Z) | analyzer evaluation | precision 0.923, recall 0.857, pair accuracy 0.875; misses listed |
| `evidence/phase-03-design-demo.log` (18:14:22Z) | `scripts/demo-design.sh` (real server, no provider) | Governance, BRD ingestion (8 chunks, confidential) and 3 approved skills all succeeded. `brd.sections` ran. The workflow **failed at `analyse`: no model provider**. Audit chain valid (65 records). 0 key/token literals in the log. |
| `evidence/phase-03-api-smoke.log` (18:13:55Z) | Phase 1 smoke (regression) | SMOKE PASSED; migrations `[1, 2, 3]` |
| `evidence/phase-03-knowledge-demo-regression.log` (18:14:03Z) | Phase 2 demo (regression) | DEMO COMPLETE (live model: false) |

Test counts:

| Phase | Tests |
|---|---|
| 1 | 140 |
| 2 | 41 |
| 3 | 33 |
| **Total** | **214** |

The 33 Phase 3 tests by area:

| Area | Tests |
|---|---|
| Requirements (unit 3, integration 6, evaluation 1) | 10 |
| Architecture (unit 1, integration 8) | 9 |
| Engineering tools | 2 |
| Engine design flow | 5 |
| Governance | 1 |
| Design demo and security | 3 |
| Fitness | 1 |
| API | 1 |
| CLI | 1 |

The real-server logs were produced after the last production-code change. The only later change is the new test file `crates/eaeh-engineering/tests/tools.rs`, which the final CI run includes.

## 9. Installation, configuration, startup, shutdown, usage

The procedures are unchanged from Phase 2; see `docs/OPERATIONS.md`, which has a new section *Requirements and architecture design (Phase 3)*. Phase 3 additions:
- **Configuration:** the optional `[requirements]` analyzer section.
- **Prerequisites before design workflows:**
  - Approve the three Phase 3 skills.
  - Ingest the BRD with `eaeh brd ingest`.
  - Enable a model provider, after the data-protection review (A-33).
- **Commands:**
  - Start with `eaeh workflow start --definition engineering.design --project P --input '{"brd":"<citation>"}'`.
  - Review with `eaeh artifact export`.
  - Decide with `eaeh approval decide … --decision approve|request_changes|reject`.
  - Query with `eaeh trace`.

## 10. Security and governance assessment

The threat model adds T41–T50. All of the following are covered by tests:
- **BRD injection:** BRD text reaches agents only as fenced data, and fs.write was refused by the gateway.
- **Cross-project BRD isolation.**
- **Fabricated sources:** sources must be retrieved citations.
- **Diagram and Markdown injection.**
- **Approval of something other than what was reviewed:** gates bind to the bundle manifest hash.
- **Ignored mandatory standards.**
- **Unbounded revisions.**
- **Misuse of `request_changes`:** limited to workflow gates, comments required, same SoD checks.
- **Forged or unapproved trace links.**
- **Quality-gate bypass.**

Residual risks:

| Risk | Detail | Action |
|---|---|---|
| Model content quality (medium) | Drafts can still be wrong or subtly unsupported while passing structural checks. Every stage is human-approved. | Citation support is not checked (A-36). |
| Egress of BRDs (medium) | BRDs are `confidential`, and the baseline policy allows confidential content to go to an external provider. | A data-protection decision is needed before live use (A-33). |
| Heuristic analyzer | Misses semantic issues. | A-37 |
| Trace query scale | In-memory scan of up to 20,000 links per project. | A-42 |

## 11. Known limitations, defects, risks and technical debt

- **ED-L1…ED-L6** (`engineering-design.md`):
  - no real-model verification;
  - lexical heuristics only;
  - Markdown/text BRDs only, and the `reference` + `brd` tag deviation;
  - single-shot generation with no streaming or per-section generation;
  - trace query limits;
  - no review UI.
- **Plan deviation:** no dedicated `brd` source kind (ADR-0018, A-39).
- **Open defects:** none known.
- **Technical debt:**
  - Phase 3 agents and workflows are file-loaded (no admin API, A-21).
  - Gates are fixed in code (`default_gates`) rather than configuration.
  - Carry-overs A-08, A-16, A-18, A-20, A-22, A-23, A-26, A-28 and A-29 were not addressed in Phase 3.

## 12. Next-phase scope, dependencies and acceptance criteria

**Scope:** Phase 4 is multi-agent software development (master §31); see ROADMAP.

**Dependencies:**
- the Phase 3 exit decision (A-38);
- **A-30 live run** (critical: Phase 4 consumes Phase 3 designs);
- real standards and reference architectures (A-09);
- the egress review (A-33);
- Linux CI with container isolation (A-08, A-26), because Phase 4 executes generated code;
- streaming and per-section generation (A-40).

**Proposed Phase 4 acceptance themes:**
- code generated only from approved LLDs, with traceability extended to code artifacts;
- isolated build and test execution;
- review gates for code;
- no regression of the Phase 1–3 controls.

## 13. Action register (Phase 3 relevant; full register in `ACTION-ITEMS.md`)

| ID | Description | Priority | Status |
|---|---|---|---|
| A-27 | Commit Phase 1–3 work | High | Open (not authorised) |
| A-30 | Live run of adapter + `engineering.design` | **Critical** before relying on Phase 3 output | Open – authorised, not executed |
| A-33 | Egress / data-protection review (BRDs are confidential) | High | Open |
| A-37 | Independent evaluation sets (analyzer, generated designs) | Medium | Open |
| A-38 | Phase 3 exit decision | High | Open |
| A-39 | Dedicated `brd` source kind | Low | Open |
| A-40 | Streaming / per-section generation | Medium | Open |
| A-41 | Review UX for bundles | Low | Open |
| A-42 | Version-aware, scalable trace queries | Low | Open |
| A-09 | Real enterprise standards and reference architectures | High | Open |

## 14. Phase exit decision and rationale

**Proposed: READY WITH ACCEPTED EXCEPTIONS**

| Exception | Description | Proposed handling |
|---|---|---|
| E-1, E-2, E-3, E-4 | Carried from Phases 1–2 and already accepted | Unchanged |
| **E-5 (new)** | No real-model run; the quality of generated requirements, HLD and LLD is unverified (AC3-20 not implemented). | Accept for Phase 3 closure only if A-30 is completed **before** Phase 4 relies on Phase 3 output. |
| **E-6 (new)** | The requirements analyzer is heuristic, with an implementer-labelled baseline (precision 0.923, recall 0.857, pair accuracy 0.875). | Accept. Every stage is human-approved; A-37 tracks an independent evaluation. |

**Rationale:**
- Every Phase 3 control (validation, rendering, gates, revisions, traceability, isolation and injection resistance) is implemented and verified by 33 new automated tests and the end-to-end demo, with all 181 earlier tests still green.
- What is missing is evidence about a real model's output, which needs your key and spend.
- If you consider real-model evidence a precondition for this phase, the correct decision is **NOT READY – ACTION REQUIRED (A-30)**. Running `EAEH_DEMO_LIVE=1 EAEH_ANTHROPIC_KEY_FILE=… scripts/demo-design.sh` would close it.

**Please decide:**
1. Accept or reject E-5 and E-6 and the exit decision (A-38).
2. Whether and when to perform the live run (A-30).
3. Whether to authorise committing Phases 1–3 (A-27).
4. Whether to authorise Phase 4. Phase 4 has not been started.
