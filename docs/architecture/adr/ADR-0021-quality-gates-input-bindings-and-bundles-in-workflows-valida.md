# ADR-0021: Quality gates, input bindings and bundles in workflows; validators in the correction loop

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-12 |
| Phase | 3 – Requirements Engineering and Architecture Generation |
| Deciders | Product owner (approval of PHASE-03-PLAN), Principal architect |

## Context
Deterministic validators existed (Phase 1) but no workflow ran them; agent tasks could not receive specific earlier outputs; a state could store only one artifact, yet an HLD consists of a document, diagrams and ADRs that must be approved together.

## Decision
* **Quality gates on work states** (`quality_gate: {gate, context}`): gates are platform configuration (`requirements`, `hld`, `lld`; block on error; semantic validators can never block alone); definitions only reference them (unknown gate ⇒ registration fails). The gate runs before the output is stored; blocking findings fail the state (fail closed), are audited (`workflow.quality_gate`) and emit `validation.failed`. The platform adds `standards` (mandatory standards supplied to the producing agent) and `knowledge` to the gate context. Results are recorded in artifact provenance.
* **Validators in the agent correction loop** (`output_validators` in agent definitions): blocking findings are fed back as platform feedback within the agent's correction budget; a missing validation engine or validator fails closed.
* **Bindings** (`$output`, `$output` + `pointer`, `$input`, `$artifact`) for agent `inputs`, tool-step inputs and gate context; resolved deterministically by the engine; bindings may only reference work states (validated at registration).
* **Bundles**: a work state with `bundle: true` stores every file of `{files: {path: text}}` as artifact `<name>/<path>` and a manifest (file artifact ids + hashes) as `<name>`; approval gates bind to the manifest hash, i.e. to every file.
* The workflow engine now depends on `eaeh-validation` (pure); it still has no dependency on agents, models, knowledge or tools (fitness).
* The agent runtime also shows the model its registered output schema (platform configuration) so live models know the exact contract.

## Consequences
Deterministic controls are enforced at two points (agent loop for fast correction, engine gate as the fail-closed backstop).

## Alternatives considered
Gates as workflow-defined validator lists (rejected: a definition could weaken a gate); expression language for bindings (rejected: unnecessary power).
