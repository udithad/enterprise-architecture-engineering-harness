# Requirements Engineering and Architecture Generation (Phase 3)

Decisions: ADR-0018 (requirements model and analysis), ADR-0019 (HLD/LLD models and
rendering), ADR-0020 (request changes, bounded revisions), ADR-0021 (quality gates,
bindings, bundles, validators in the correction loop), ADR-0022 (traceability).

Principle: **intelligent components draft; deterministic components structure, check,
render and gate; humans approve.** Every generated document is a typed JSON model. The
platform parses it strictly, validates it, renders Markdown, Mermaid and ADRs from it, and
computes the traceability links.

## Components

| Crate / asset | Responsibility | Key interfaces |
|---|---|---|
| `eaeh-requirements` (pure) | Requirements model; structural validation; ambiguity, conflict, duplicate and missing-requirement analysis; BRD coverage; traceability; Markdown rendering | `RequirementSet`, `analyze`, `RequirementsValidator` (`requirements.set`), `trace_links`, `render_markdown` |
| `eaeh-architecture` (pure) | HLD/LLD models; integrity, coverage, refinement and compliance validation; deterministic Markdown, Mermaid and ADR rendering; trace links | `Hld`, `Lld`, `HldValidator` (`architecture.hld`), `LldValidator` (`architecture.lld`), `render_hld`, `render_lld`, `hld_trace`, `lld_trace` |
| `eaeh-engineering` | Governed read-only tools; validator registration; default quality gates | `BrdSectionsTool` (`brd.sections`), `RequirementsRenderTool` (`requirements.render`), `ArchitectureRenderTool` (`architecture.render`), `DesignCheckTool` (`design.check`), `register_validators`, `default_gates` |
| `eaeh-knowledge` (extended) | Whole-document reads within the caller's visibility | `KnowledgeService::read_document`, `KnowledgeReadTool` (`knowledge.read`) |
| `eaeh-workflow` (extended) | `changes_requested` trigger and `max_revisions`; bindings; quality gates on states; bundles; trace-link recording and query | `QualityGateRef`, `resolve_bindings`, `WorkflowEngine::{with_quality_gates, trace}` |
| `eaeh-governance` (extended) | `request_changes` decision and `changes_requested` status | `ApprovalDecision::RequestChanges` |
| `eaeh-agents` (extended) | `output_validators` in the correction loop; output schema shown to the model; standards recorded | `OutputValidatorRef`, `AgentResult.standards` |
| Configuration | Agents `business-analyst`, `requirements-engineer`, `solution-architect`, `software-designer`; prompts; skills `requirements-analysis`, `hld-design`, `lld-design` (`config/skills`, human approval required); workflows `requirements.analysis` and `engineering.design` | `config/agents`, `prompts`, `workflows` |

## `engineering.design` workflow

```
brd (tool brd.sections, input $input:/brd)
 → analyse  (business-analyst; inputs.brd; validator requirements.set in the loop; gate "requirements")
 → refine   (requirements-engineer; inputs.brd + draft; same validator and gate)
 → render_requirements (tool requirements.render; bundle design/requirements: requirements.md, requirements.json, analysis.json; trace)
 → approve_requirements (approver; approve → hld | request_changes → refine | reject → rejected; max_revisions 3)
 → hld      (solution-architect; requires approve_requirements; inputs.requirements; validator architecture.hld; gate "hld")
 → render_hld (tool architecture.render; bundle design/hld: hld.md, hld.json, diagrams/{context,components,deployment}.mmd, adr/*.md)
 → approve_hld (approve → lld | request_changes → hld | reject)
 → lld      (software-designer; requires both approvals; inputs.requirements + hld; validator architecture.lld; gate "lld")
 → render_lld (bundle design/lld: lld.md, lld.json, data-model, sequence-*, component-internals, deployment, adr/*)
 → approve_lld → done
```

`requirements.analysis` contains only the requirements stages.

Controls on every agent step:
- Context: mandatory standards at `enterprise_policy` trust and the approved skill at `skill` trust. The BRD (via `knowledge.read`) and earlier outputs arrive as fenced data.
- The agent runtime checks the agent's output schema, the skill's output contract, the citation validator and the domain validator. Blocking findings go back to the agent as correction feedback.
- The engine re-runs the quality gate as a fail-closed backstop before storing anything.
- A human approval is required, bound to the bundle manifest hash.
- Trace links are recorded only after approval.

## Deterministic checks

| Check | Requirements | HLD | LLD |
|---|---|---|---|
| Strict parse, ids, references | ✔ | ✔ | ✔ |
| Sources/citations verified against retrieval | sources ⊆ citations, chunk format | standards ⊆ citations | standards ⊆ citations |
| Coverage | every BRD section covered or excluded | every Must requirement allocated | every Must requirement realised |
| Ambiguity / conflicts / duplicates / gaps | lexicon, TBD, measurability, contradictions, numeric conflicts, NFR checklist | – | – |
| Refinement | – | – | components refine HLD; every buildable HLD component designed |
| Decisions / risks / trade-offs | – | alternatives, consequences; risks with mitigation; trade-off choice ∈ options | decisions; risks |
| Standards compliance | – | every mandatory standard addressed; deviation ⇒ justification + decision | same |

## Configuration

```toml
[requirements]               # optional; defaults shown in AnalysisConfig::default()
nfr_categories = ["performance_efficiency", "compatibility", "usability", "reliability",
                  "security", "maintainability", "portability"]
duplicate_threshold = 0.9
contradiction_threshold = 0.8
numeric_conflict_threshold = 0.6
# weak_phrases = [...], unresolved_markers = [...]
```

## Measured baseline

From the analyzer evaluation (`evidence/phase-03-analyzer-eval.log`):
- **Ambiguity:** precision 0.923, recall 0.857 on 24 labelled statements.
- **Conflict/duplicate pairs:** accuracy 0.875 on 8 pairs.

Known misses: "timely", "large", one semantic conflict, and a false positive on "fast-track".

## Known limitations

| Id | Limitation |
|---|---|
| ED-L1 | Real-model generation quality is not verified. The live run was authorised (P3-D1), but no key was provided, so it was not executed (A-30). |
| ED-L2 | Lexical heuristics only: they do not detect semantic contradictions or missing domain knowledge (A-37). |
| ED-L3 | BRD formats are Markdown and text only (A-35). BRDs use source kind `reference` plus the `brd` tag, a deviation from the plan (A-39). |
| ED-L4 | Each design is one model output. Very large systems may exceed output limits; per-section generation and streaming are not implemented (A-40). |
| ED-L5 | Trace queries scan the project's links in memory (limit 20,000 rows) and return links from every approved version (A-42). |
| ED-L6 | Reviewers see rendered Markdown and Mermaid text. No image rendering or review UI. |
