# EAEH Architecture Overview

> Status: reflects the implementation at the end of **Phase 3 – Requirements Engineering and Architecture Generation** (Phases 1–2 plus Phase 3 extensions).
> Decisions are recorded in [`adr/`](adr/README.md). Phase scope and status are in
> [`../implementation/ROADMAP.md`](../implementation/ROADMAP.md).

EAEH is a Rust-first, modular-monolith engineering harness. Deterministic components
(workflow engine, policy engine, authorization, approvals, budgets) are **authoritative**;
intelligent components (agents, planner, models) **propose** and act only through governed
gateways.

## Logical layers → crates

| Layer | Responsibility | Crates |
|---|---|---|
| 1 – Workflows & experience | APIs, CLI, workflow definitions | `eaeh-api`, `eaeh-cli`, `workflows/` |
| 2 – Intelligent execution | Agents, delegation, models, prompts, context, knowledge, skills | `eaeh-agents`, `eaeh-intelligence`, `eaeh-knowledge`, `eaeh-skills` |
| 2a – Engineering design (Phase 3) | Deterministic requirements and HLD/LLD models, validators, renderers; design tools | `eaeh-requirements`, `eaeh-architecture` (pure), `eaeh-engineering` |
| 3 – Harness runtime & extensibility | Hooks, middleware, interceptors, plugins, tool gateway, workspaces, composition, supervision | `eaeh-extensibility`, `eaeh-tools`, `eaeh-workspace`, `eaeh-runtime` |
| 4 – Deterministic control & governance | Workflow engine, policy, authorization, approvals, budgets, validation | `eaeh-workflow`, `eaeh-governance`, `eaeh-validation` |
| 5 – Infrastructure | Core types, persistence, audit, events, isolation, LLM provider adapter | `eaeh-core`, `eaeh-store`, `eaeh-events`, `eaeh-isolation`, `eaeh-provider-anthropic` |
| Test support | Labelled test doubles (dev-only) | `eaeh-testkit`, `tests/` (`eaeh-acceptance`) |

Crate dependencies are enforced by `tests/tests/fitness.rs` (ADR-0002):

```
                 eaeh-core
        ┌───────────┼──────────────┐
   eaeh-store   eaeh-isolation  eaeh-validation
        │
   eaeh-events
        │
 eaeh-governance ──────────────┐
        │                      │
 eaeh-extensibility            │
   │        │         │        │
eaeh-     eaeh-      eaeh-     │
workspace intelligence tools ◄─┘ (+isolation, workspace, validation)
   │        │         │
   │        │  eaeh-knowledge, eaeh-skills (Phase 2; above tools & intelligence)
   │        │  eaeh-requirements → eaeh-architecture (Phase 3; core + validation only)
   │        │  eaeh-engineering (tools over the above + knowledge; used only by runtime)
   │        └──► eaeh-agents ◄┘
   │
 eaeh-workflow  (governance + extensibility + workspace + validation — no agents/intelligence)
        \            /
        eaeh-runtime (composition root: wires executors, planner, providers, plugins,
                      knowledge, skills, opt-in eaeh-provider-anthropic)
             │
          eaeh-api ── eaeh-cli
```

## Key runtime flows

1. **Governed tool invocation** – `ToolGateway::invoke`: allow-list → authorization →
   autonomy ceiling → input schema → policy → approval binding → isolation availability →
   rate limit → budget → *then* hooks → middleware → isolated execution → output schema →
   interceptors (redaction, size) → audit + `ToolInvoked` event (one transaction). Output
   is returned as `Untrusted<Value>`. See [agents-and-tools.md](agents-and-tools.md).
2. **Workflow with approval gate** – `WorkflowEngine::drive` executes work states through
   `TaskExecutor`, stores outputs as versioned artifacts, requests approval bound to the
   artifact hash, blocks in `waiting_approval`, resumes only after a human approver's
   decision recorded by `ApprovalManager`. States can require prior gate approval
   independently of the transition graph. See
   [runtime-and-orchestration.md](runtime-and-orchestration.md).
3. **Hybrid planning** – `planned` states ask a `Planner` (an agent with capability `plan`)
   for a proposal; the engine validates kinds, step count and per-step policy before any
   step runs.
4. **Agent execution** – `AgentRuntime` instantiates an agent identity (role-less,
   project-bound), assembles trust-layered context, calls the governed `ModelClient`,
   routes proposed tool calls through the gateway, feeds results back as untrusted data,
   validates the final output against the definition's schema, and terminates.

## Documents

| Topic | Document |
|---|---|
| Component catalogue (26 master components) | [components.md](components.md) |
| Workflow engine, hybrid orchestration, persistence, recovery, supervision, budgets, observability | [runtime-and-orchestration.md](runtime-and-orchestration.md) |
| Agent runtime, delegation, tool registry & gateway | [agents-and-tools.md](agents-and-tools.md) |
| Hooks, middleware, interceptors, events, plugins | [extensibility.md](extensibility.md) |
| Execution isolation | [isolation.md](isolation.md) |
| Context, prompts, models | [intelligence.md](intelligence.md) |
| Enterprise knowledge, retrieval, skills (Phase 2) | [knowledge-and-skills.md](knowledge-and-skills.md) |
| Requirements engineering, HLD/LLD generation, traceability (Phase 3) | [engineering-design.md](engineering-design.md) |
| Identity, authorization, approvals, secrets, prompt security | [security.md](security.md) |
| Threat model | [security/THREAT-MODEL.md](security/THREAT-MODEL.md) |
| Data model & persistence | [data.md](data.md) |
| External integrations | [integrations.md](integrations.md) |
| Operations: install, configure, run, API & CLI reference | [../OPERATIONS.md](../OPERATIONS.md) |
