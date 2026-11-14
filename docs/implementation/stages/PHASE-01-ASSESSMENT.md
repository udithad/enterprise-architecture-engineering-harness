# PHASE-01 – Repository Assessment and Phase 1 Implementation Proposal

| Field | Value |
|---|---|
| Phase | 1 – Core Harness Foundation |
| Document type | Read-only repository assessment (per `claude-instructions.md` §43) |
| Date | 2026-11-12 |
| Assessed commit | `1527c09` (branch `main`, clean working tree) |
| Status | **APPROVED** 2026-11-12 (see approval record below) |

## Approval record

Approved by the product owner on 2026-11-12 with decisions: D-01 SQLite/SQLx with
repository abstractions; D-02 in-house deterministic policy engine; D-03 trusted in-process
extensions only (no dynamic loading of untrusted libraries); D-04 provider abstraction with
test-only scripted provider; D-05 OpenTelemetry optional (structured logging, tracing
foundations and correlation ids mandatory); D-06 currently supported stable toolchain
(pinned: 1.99.1); D-07 Docker or Podman, whichever is available, with tested isolation.
Clarifications 1–6 (plugin trust, declared isolation capabilities, tested container
claims, credential provisioning/rotation/revocation without default credentials,
fitness-enforced dependency graph, no future-phase capabilities) are recorded in
ADR-0002, 0008, 0009 and 0011. This document is otherwise preserved as written before
approval.

---

## 1. Repository Structure (observed, not assumed)

```
enterprise-architecture-engineering-harness/
├── LICENSE                  Apache License 2.0 (201 lines)
├── README.md                Single heading line, no content
└── claude-instructions.md   Master architecture & implementation instruction (1,346 lines)
```

Git history: 3 commits (`7982f9a` Initial commit → `8f67cc8` instructions added → `1527c09` instructions revised). No branches other than `main`, no tags.

**Not present:** source code, `Cargo.toml`/build manifests, configuration, tests, CI pipelines, `docs/`, ADRs, dependency lockfiles, container files, scripts.

### 1.1 Local environment (observed via read-only commands)

| Tool | Observed | Relevance |
|---|---|---|
| rustc / cargo | 1.86.0 (2025-03-31), stable, aarch64-apple-darwin | Core language. Supports edition 2024. Older than current stable – see Risk R-02. |
| clippy / rustfmt | 0.1.86 / 1.8.0 | Lint and format gates available. |
| cargo-audit / cargo-deny | **Not installed** | Needed for dependency/licence/vulnerability gates (A-07). |
| sqlite3 | 3.51.0 | Embedded persistence candidate (bundled by crate anyway). |
| docker | CLI 28.2.2, **daemon not running** | Container isolation provider testing is blocked until a daemon is available. |
| podman | 5.5.1 (machine status not checked) | Alternative OCI runtime for container isolation provider. |
| git | 2.54.0 | Workspace/worktree support. |
| OS | macOS (Darwin 25.6.0), arm64, 8 CPUs | Affects process-isolation capabilities (see §13, R-03). |

## 2. Existing Technologies and Architectural Patterns

None. There is no implemented technology or pattern in the repository. The only architectural input is the approved decision set in `claude-instructions.md` §3:

| Dimension | Approved decision | Carried into this proposal |
|---|---|---|
| Orchestration | Hybrid | Yes – deterministic workflow engine (authoritative) + intelligent agent orchestrator (advisory) |
| Agent instantiation | Dynamic | Yes – definitions in registry, instances created/terminated per task |
| Execution environment | Isolated | Yes – `ExecutionProvider` abstraction with process and container implementations |
| Software development | Multi-agent | Foundation only in Phase 1 (communication, delegation contracts) |
| Human approvals | Gated | Yes – enforced in workflow engine and API authorization, not prompts |
| Core implementation | Rust-first (Option A) | Yes – Cargo workspace, all core components in Rust |

## 3. Existing Modules and Capabilities

None exist. All 26 core platform components (§5 of the master instructions) are currently **NOT IMPLEMENTED**.

## 4. Reusable Components

| Candidate | Reusable? | Notes |
|---|---|---|
| Existing source code | N/A | None exists. |
| `LICENSE` (Apache-2.0) | Yes | Constrains dependency licences: permissive (MIT/Apache-2.0/BSD/ISC/Zlib/Unicode) preferred; copyleft dependencies require explicit approval. |
| `claude-instructions.md` | Yes | Authoritative requirements source; Phase 1 acceptance criteria below trace back to its sections. |
| Rust ecosystem crates | Yes, subject to evaluation | See §15 dependency evaluation. |

## 5. Gaps Against the EAEH Vision

The entire EAEH vision is a gap. Phase 1 closes the *foundation* gaps only. Gaps that are deliberately **out of Phase 1 scope** (per §28):

- Complete knowledge management and skills management (Phase 2).
- BRD analysis, requirements engineering, HLD/LLD generation (Phase 3).
- Full multi-agent software development workflows, language adapters (Phase 4).
- Comprehensive test orchestration and evaluation (Phase 5).
- Git provider / CI/CD integration (Phase 6).
- Release and deployment management (Phase 7).
- Distributed execution, HA, advanced IAM, federation (Phase 8).

Foundational security, authorization, isolation, secret protection and audit **are** in Phase 1 scope (§35).

## 6. Proposed Target Architecture

A single Cargo workspace, deployed as one modular binary (`eaeh`) that can run as a CLI or as an HTTP API server. Logical layers map to crates; crates enforce compile-time boundaries. No microservices, no external message broker, no external database server in Phase 1.

```
Layer 1  Workflows & Experience     eaeh-api, eaeh-cli
Layer 2  Intelligent Execution      eaeh-agents, eaeh-intelligence (LLM, prompts, context)
Layer 3  Harness Runtime & Ext.     eaeh-extensibility (hooks, middleware, interceptors, plugins),
                                    eaeh-tools, eaeh-workspace, eaeh-runtime (composition, supervision)
Layer 4  Control & Governance       eaeh-workflow (engine + hybrid orchestration), eaeh-governance
                                    (policy, authz, approvals, budgets), eaeh-validation
Layer 5  Infrastructure             eaeh-store (SQLite/SQLx, migrations, audit chain), eaeh-events,
                                    eaeh-isolation, eaeh-core (ids, errors, identity, secrets, clock)
```

### 6.1 Allowed dependency direction (enforced by an architecture fitness test)

```
eaeh-core
  ↑
eaeh-store, eaeh-events
  ↑
eaeh-governance, eaeh-extensibility
  ↑
eaeh-isolation, eaeh-workspace, eaeh-tools, eaeh-intelligence, eaeh-validation
  ↑
eaeh-agents
  ↑
eaeh-workflow
  ↑
eaeh-runtime   (composition root – the only crate that wires concrete implementations)
  ↑
eaeh-api, eaeh-cli
```

Rules: no cycles, no upward dependencies, governance never depends on agents/intelligence (an LLM component can never be a dependency of an authorization decision), `eaeh-api`/`eaeh-cli` never bypass `eaeh-runtime` to reach stores directly.

## 7. Component / Module Breakdown

Mapping of the 26 master components to Phase 1 crates. "Phase 1 depth" states what will actually be built; anything else is explicitly deferred.

| # | Master component | Crate | Phase 1 depth |
|---|---|---|---|
| 1 | Core Runtime | `eaeh-core`, `eaeh-runtime` | Typed IDs, error taxonomy, `Principal`, scopes, clock, config, harness builder |
| 2 | Hybrid Orchestration Engine | `eaeh-workflow` | Deterministic engine + `Planner` trait; planner output is a *proposal* validated by engine + policy |
| 3 | Workflow Management | `eaeh-workflow` | Declarative workflow definitions (states, transitions, gates), durable instances, checkpoints, retry/timeout/cancel, recovery |
| 4 | Agent Runtime & Management | `eaeh-agents` | Definitions, versioned registry, capability discovery, dynamic instantiate/execute/terminate, autonomy levels |
| 5 | Agent Communication | `eaeh-agents` | Typed messages, delegation with depth/cycle/fan-out limits, correlation, audit |
| 6 | Execution Environment Manager | `eaeh-isolation` | `ExecutionProvider` trait; restricted-process provider; OCI container provider (Docker/Podman CLI) |
| 7 | Hook Framework | `eaeh-extensibility` | Typed hook points for all §6 categories, priority ordering, filters, timeouts, block/warn/continue policy, enable/disable, tracing |
| 8 | Middleware & Interceptors | `eaeh-extensibility` | Generic ordered middleware pipeline (around-call) + interceptors (inspect/transform at boundary); distinct traits |
| 9 | Event Management | `eaeh-events` | Versioned event envelope, in-process bus, durable outbox, idempotent consumers, retry + dead-letter |
| 10 | Plugin & Extension Mgmt | `eaeh-extensibility` | Manifest schema (3 kinds), integrity digest + ed25519 signature check, registry, lifecycle, permissions; in-process host only |
| 11 | Tool Gateway & Registry | `eaeh-tools` | Registry, JSON-Schema I/O contracts, risk classes, authz + policy + budget + hooks + middleware, isolation dispatch, audit |
| 12 | Context Engineering | `eaeh-intelligence` | Context assembly with trust levels, provenance, token budget, permission-aware scoping (no knowledge store yet) |
| 13 | Prompt Management | `eaeh-intelligence` | Versioned prompt templates, typed variables, explicit precedence, untrusted-content fencing |
| 14 | LLM Provider & Model Mgmt | `eaeh-intelligence` | Provider trait, provider/model registry, routing by capability/cost, fallback/retry, usage tracking |
| 15 | Enterprise Knowledge Mgmt | – | **Deferred to Phase 2** (context contracts accept knowledge items) |
| 16 | Skills Management | – | **Deferred to Phase 2** (agent definitions reference skill IDs as data only) |
| 17 | Validation Framework | `eaeh-validation` | `Validator` trait, severity, evidence, schema validator, quality gate aggregation |
| 18 | Feedback & Correction Engine | `eaeh-runtime` | Bounded correction-attempt counter + escalation only (full engine Phase 4/5) |
| 19 | Workspace & Artifact Mgmt | `eaeh-workspace` | Workspace lifecycle, path confinement, ownership locks, content-addressed artifacts with provenance |
| 20 | Governance & Policy Engine | `eaeh-governance` | Deterministic, default-deny, deny-overrides rule engine; versioned policies; decision records |
| 21 | Human Approval Mgmt | `eaeh-governance` | Approval requests/decisions, role requirements, separation of duties, no self-approval, revision on reject |
| 22 | Execution Budget & Supervision | `eaeh-governance`, `eaeh-runtime` | Budgets (duration, tokens, cost, tool calls, concurrency, delegation depth, corrections); stalled/repeat-failure detection |
| 23 | Observability & Audit | `eaeh-store`, `eaeh-runtime` | `tracing` structured logs + spans, correlation/causation IDs, hash-chained audit log with verification |
| 24 | Project & Tenant Mgmt | `eaeh-core`, `eaeh-store` | Tenant/project IDs on every scoped entity; scope checks in authz and store queries |
| 25 | Engineering Integration Mgmt | – | **Deferred** (Phase 6); adapter trait style established by isolation/LLM |
| 26 | API, CLI & User Interaction | `eaeh-api`, `eaeh-cli` | Versioned `/v1` HTTP API (Axum) with token auth; `eaeh` CLI |

Test support: `eaeh-testkit` (dev-dependency only) providing fixtures and **explicitly labelled test doubles** (e.g. `ScriptedModelProvider`). Test doubles are never registered by the production composition root.

## 8. Interfaces and Data Flows

### 8.1 Key interfaces (Rust traits, indicative)

```rust
// eaeh-isolation
trait ExecutionProvider: Send + Sync {
    fn id(&self) -> &ProviderId;
    fn capabilities(&self) -> IsolationCapabilities;      // fs, network, cpu, mem, pids – what it can actually enforce
    async fn execute(&self, spec: ExecutionSpec) -> Result<ExecutionOutcome, IsolationError>;
}

// eaeh-governance
trait PolicyEngine: Send + Sync {
    fn evaluate(&self, req: &PolicyRequest) -> PolicyDecision;   // deterministic, no I/O, no LLM
}
trait Authorizer: Send + Sync {
    fn authorize(&self, principal: &Principal, action: Action, resource: &ResourceRef) -> AuthzDecision;
}

// eaeh-intelligence
trait ModelProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ModelError>;
}

// eaeh-tools
trait Tool: Send + Sync {
    fn descriptor(&self) -> &ToolDescriptor;     // id, version, schemas, risk class, required capabilities
    async fn invoke(&self, ctx: ToolContext, input: serde_json::Value) -> Result<ToolOutput, ToolError>;
}

// eaeh-extensibility
trait Hook<E: HookEvent>: Send + Sync { async fn on(&self, ev: &E) -> HookResult; }
trait Middleware<Req, Res>: Send + Sync { async fn handle(&self, req: Req, next: Next<'_, Req, Res>) -> Result<Res, Error>; }
trait Interceptor<T>: Send + Sync { fn intercept(&self, value: T) -> Result<T, Error>; }

// eaeh-validation
trait Validator<T>: Send + Sync { fn validate(&self, subject: &T) -> ValidationReport; }

// eaeh-workflow
trait Planner: Send + Sync {                       // intelligent orchestration – advisory only
    async fn propose(&self, task: &TaskSpec, ctx: &PlanningContext) -> Result<PlanProposal, PlanError>;
}
```

### 8.2 Primary data flow – governed tool invocation by an agent

```
Agent instance (LLM-driven)
  └─▶ ToolCallRequest (typed, from model output — UNTRUSTED)
        └─▶ Tool Gateway
              1. Schema-validate input                       (deterministic)
              2. Authorize: agent identity + autonomy level  (deterministic, Authorizer)
              3. Policy evaluation: risk class, scope        (deterministic, PolicyEngine)
              4. Approval check if risk requires it          (ApprovalManager)
              5. Budget reserve (tool calls / time)          (BudgetManager)
              6. before_tool_invocation hooks                (cannot override 2–5)
              7. Middleware pipeline (redaction, tracing, rate limit)
              8. Dispatch → ExecutionProvider (isolated)
              9. Output schema validation + interceptors (normalize, mark untrusted)
             10. after_tool_invocation / on_tool_failure hooks
             11. Audit record (hash chained) + ToolInvoked event (outbox)
```

Steps 1–5 are executed by trusted platform code **before** hooks run; disabling or failing hooks cannot skip them (§6 requirement).

### 8.3 Workflow with approval gate

```
Client ─▶ API /v1/workflows (authn → authz) ─▶ WorkflowEngine.start
WorkflowEngine: state=Planning ─▶ Planner.propose (advisory) ─▶ engine validates plan against
               workflow definition + policy ─▶ tasks scheduled ─▶ AgentRuntime executes
               ─▶ state=AwaitingApproval (gate) ── checkpoint persisted ──
Approver ─▶ API /v1/approvals/{id}/decision (authn → role check → separation of duties →
            not the artifact author/agent) ─▶ ApprovalManager records decision ─▶
            engine transition (Approved → next stage | Rejected → Revision, history kept)
```

## 9. Hooks, Middleware, Event and Plugin Architecture

| Mechanism | Responsibility | Can change outcome? | Failure semantics |
|---|---|---|---|
| **Hook** | React to a named lifecycle point (observe, enrich, veto where permitted) | Only `block` hooks at `before_*` points, and only *more* restrictive | Per-registration policy: `Block` (abort operation), `Warn` (log + audit, continue), `Continue` (silent). Timeout counts as failure. |
| **Middleware** | Cross-cutting around-processing of a request pipeline (authn, tracing, rate limit, cost, redaction) | Yes, via ordered chain; may short-circuit | Error propagates to caller; deterministic order by explicit `order` value then registration ID |
| **Interceptor** | Inspect/transform a value crossing a boundary (e.g. normalize tool output, redact secrets, tag untrusted) | Transforms data only | Error aborts the boundary crossing |
| **Event** | Decoupled notification of facts that happened (`AgentCreated`, `ToolInvoked`, …) | No (after-the-fact) | Durable via outbox; retry with backoff; dead-letter after N attempts; consumer idempotency by event ID |
| **Plugin** | Packaged extension contributing hooks/middleware/tools/providers | Only via the above contracts | Lifecycle state machine: Registered → Validated → Enabled ⇄ Disabled → Retired; health status |

- Hook catalogue: all categories listed in §6 (agent, task/workflow, LLM, tool, knowledge/context, engineering) are defined as typed hook points. Phase 1 **emits** agent, task/workflow, LLM, tool, context and artifact/file hooks from implemented code paths. Build/test/commit/PR/deployment hook points are defined as contracts but have no emitting code path until Phases 4–7 (reported as such, not as implemented behaviour).
- Event envelope: `event_id`, `event_type`, `schema_version`, `occurred_at`, `tenant_id`, `project_id`, `correlation_id`, `causation_id`, `producer`, `payload`.
- Plugin manifests: `id`, `version`, `kind` (`in_process` | `wasm` | `external_process`), `eaeh_api_version` compatibility range, `dependencies`, `requested_capabilities`, `config_schema`, `resource_limits`, `digest` (SHA-256), optional `signature` (ed25519) verified against a configured trusted-key allowlist. Plugins receive only capabilities granted by policy and invoke tools through the gateway – there is no privileged handle.
- Phase 1 extension hosts: **in-process trusted Rust extensions only**. WASM and external-process kinds get manifest, validation and registry support but no execution host (proposed deferral – Decision D-03).

## 10. Agent Runtime and Tool Gateway Design

**Agent definition** (versioned, stored, validated): `id`, `version`, `purpose`, `capabilities[]`, `skill_refs[]` (opaque in Phase 1), `model_preferences` (capability requirements, cost tier), `allowed_tools[]`, `required_context[]`, `input_schema`, `output_schema`, `autonomy_level` (0–4), `permissions`, `constraints` (budgets), `evaluation_criteria`.

**Lifecycle:** `Defined → Instantiating → Ready → Running → (Completed | Failed | Cancelled) → Terminated`, with before/after hooks at each transition. Instances are created on demand by the runtime for a task and terminated afterwards; none run continuously.

**Discovery:** capability matching (required capabilities ⊆ agent capabilities, version constraints, autonomy level ≤ task-allowed level), deterministic tie-break.

**Agent identity:** each instance gets a `Principal::Agent { instance_id, definition_id@version, tenant, project, autonomy_level }`. Authorizer treats agents as principals that can **never** hold an approver role (enforces no self-approval structurally).

**Autonomy enforcement:** autonomy level maps to a maximum tool risk class, checked in the gateway: L0 read-only; L1 read-only + proposal artifacts; L2 + workspace-modify; L3 + build/test/execute; L4 + external side effects (still subject to approval policy).

**Communication:** typed `AgentMessage { from, to, kind: Delegate|Result|Info, task_contract, correlation_id, causation_id, deadline }`. Delegation checks: authorization, max depth, cycle detection over the delegation chain, max fan-out / concurrent instances, duplicate-task fingerprint.

**Tool risk classes:** `ReadOnly`, `WorkspaceWrite`, `BuildTest`, `Git`, `ExternalApi`, `CloudInfra`, `Deployment`. Default policy: `ReadOnly`/`WorkspaceWrite` allowed within autonomy; `ExternalApi`+ require explicit policy allow; `CloudInfra`/`Deployment` require human approval.

Phase 1 built-in tools (to exercise the gateway end-to-end, not as domain integrations): `fs.read`, `fs.write`, `fs.list` (workspace-confined) and `exec.run` (command allowlist, dispatched through `ExecutionProvider`).

## 11. Context, Prompt and Model Management Design

- **Trust levels:** `System` > `EnterprisePolicy` > `AgentInstruction` > `Skill` > `TaskInput` > `Retrieved/ToolOutput (Untrusted)`. Precedence is explicit and encoded in the assembler; untrusted content is wrapped in delimited data blocks with provenance and is never merged into instruction sections.
- **Context assembly:** items carry `source`, `version`, `scope` (tenant/project), `trust`, `token_estimate`, `freshness`. Assembler filters by scope permission, sorts by priority, enforces token budget, records a context snapshot (hash + item provenance) for traceability.
- **Prompt registry:** templates are versioned assets (files under `prompts/` loaded into the store) with typed variable schemas, validation, status (`draft`/`approved`/`retired`), and rollback to prior version. No prompt text in Rust source beyond test fixtures.
- **Model management:** provider registry, model registry (capabilities, context window, cost per token, residency tags), router selecting the cheapest model satisfying requirements (not the most expensive), fallback chain on retryable errors, per-execution record of provider + model + version + token usage + cost.
- **Real provider adapter:** proposed **not** in Phase 1 (Decision D-04). Phase 1 delivers the abstraction plus a deterministic scripted provider used only in tests.

## 12. Workflow and Persistence Strategy

- **Workflow definitions:** declarative (TOML/JSON) state machines: states, transitions, guards (policy/approval/validation), task templates, timeouts, retry policy. Validated at load (reachability, terminal states, gate presence).
- **Durability:** every state transition is a single DB transaction that writes (a) the new instance state with optimistic version check, (b) the checkpoint, (c) audit record, (d) outbox events. Recovery on startup scans non-terminal instances and resumes from the last checkpoint; in-flight tasks without a completion record are classified (retryable vs needs-human) by idempotency key.
- **Idempotency:** side-effecting operations carry idempotency keys; completed keys are recorded so replays do not repeat effects.
- **Persistence technology (proposed):** SQLite via SQLx (runtime-checked queries, no `DATABASE_URL` needed at compile time), WAL mode, explicit numbered migrations, foreign keys enforced. Repository traits keep a future PostgreSQL adapter possible without core changes (Decision D-01).
- **Audit log:** append-only table; each record stores `prev_hash` and `hash = SHA-256(prev_hash ‖ canonical_record)`; `eaeh audit verify` detects tampering/deletion.

## 13. Security Boundaries and Threat Considerations

| Trust boundary | Threats | Phase 1 mitigations |
|---|---|---|
| API caller → API | Unauthenticated access, approval bypass via direct call, cross-tenant access | Token authN (hashed tokens, extension point for OIDC), authZ on every route, approval endpoint enforces role + SoD, tenant/project scope checks |
| LLM output → runtime | Prompt injection authorizing actions, fabricated tool calls | Model output is untrusted data; all actions go through gateway authz/policy; LLMs cannot call approval APIs (agents can't hold approver role) |
| Retrieved/tool output → context | Indirect injection, instruction override | Trust-level separation, fenced data blocks, precedence rules, tests asserting system instructions unchanged |
| Tool execution → host | FS escape, network exfiltration, resource exhaustion | `ExecutionProvider` with declared enforceable capabilities; policy refuses to run a tool on a provider that cannot enforce its required isolation |
| Plugin → core | Policy bypass, privilege escalation, tampered package | Manifest digest/signature, capability grants, only gateway-mediated access, no direct store handle |
| Workspace → host FS | Path traversal, symlink escape, concurrent corruption | Canonicalized path confinement, symlink rejection, ownership locks |
| Secrets → logs/context | Leakage | `Secret<T>` type with redacted `Debug`/`Display`/`Serialize`, redaction interceptor on logs, context and tool output; tests scan captured logs |
| Agent → agent | Uncontrolled delegation, recursion, spawn storms | Depth/cycle/fan-out limits, budgets, supervision |

**Isolation honesty (macOS):** a plain child process on macOS cannot be denied network access or hard-limited on memory by portable means (`sandbox-exec` is deprecated; `RLIMIT_AS` is unreliable on Darwin). Proposed approach:

1. `RestrictedProcessProvider` – cleared environment, confined working directory, CPU-time and file-size rlimits, wall-clock timeout with process-group kill, output size cap. Declares `network: NotEnforced`, `memory: NotEnforced` on macOS.
2. `ContainerProvider` (Docker or Podman CLI) – `--network none`, `--read-only` rootfs, bind-mounted workspace only, `--memory`, `--cpus`, `--pids-limit`, `--cap-drop ALL`, `--security-opt no-new-privileges`, non-root user. Declares all capabilities enforced.
3. The gateway/policy matches a tool's required isolation against the provider's declared capabilities and **refuses** rather than silently downgrading.
4. Isolation tests actually attempt escapes (network connect, write outside mount, fork bomb within pids limit, memory exhaustion, timeout). Container tests require a running daemon; if unavailable they will be reported as **BLOCKED**, not passed.

A full threat model document (`docs/architecture/security/THREAT-MODEL.md`) will be created in Phase 1 with threats, mitigations, residual risks and test evidence.

## 14. Observability and Testing Strategy

**Observability:** `tracing` with JSON subscriber; spans for workflow, task, agent, tool, model calls carrying `correlation_id`/`causation_id`/tenant/project; redaction layer; metrics as counters/histograms exposed via `GET /v1/health` and `GET /v1/metrics` (Prometheus text format, Phase 1 basic set). OpenTelemetry export is proposed as an optional feature, not required for Phase 1 acceptance (Decision D-05).

**Testing layers:**

| Layer | Location | Tooling |
|---|---|---|
| Unit | `crates/*/src` `#[cfg(test)]` | `cargo test` |
| Component / integration | `crates/*/tests/` | `cargo test`, temp SQLite DBs, temp dirs |
| API / contract | `crates/eaeh-api/tests/` | in-process Axum router (`tower::ServiceExt::oneshot`) |
| End-to-end | `tests/e2e/` (workspace-level crate) | full harness + CLI binary (`assert_cmd`) |
| Security & governance | `tests/security/` | dedicated suite mapping to §36 scenarios |
| Isolation | `crates/eaeh-isolation/tests/` | real process/container escape attempts; container suite gated on runtime availability |
| Failure & recovery | `tests/e2e/recovery_*` | crash-simulation (drop runtime mid-workflow, reopen DB) |
| Architecture fitness | `tests/fitness/` | `cargo metadata` dependency-direction assertions, forbidden-dependency checks |
| Quality gates | `scripts/ci.sh` | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `cargo deny check` (if installed) |

Each test scenario will be documented in `docs/implementation/TEST-STRATEGY.md` with objective, prerequisites, configuration, sample data, exact command, expected behaviour, pass/fail criteria and evidence location, per §36.

## 15. Phase 1 Implementation Plan

### 15.1 Proposed dependencies (to be confirmed in ADR-0003 with licence/maturity review at baseline)

| Crate | Purpose | Licence | Rationale |
|---|---|---|---|
| `tokio` | Async runtime | MIT | De-facto standard, mature |
| `axum` + `tower` / `tower-http` | HTTP API, middleware | MIT | Tokio-team maintained |
| `serde`, `serde_json`, `toml` | Serialization, config | MIT/Apache-2.0 | Standard |
| `sqlx` (sqlite, runtime-tokio, migrate) | Persistence, migrations | MIT/Apache-2.0 | Async, migration support, Postgres path |
| `tracing`, `tracing-subscriber` | Logging/tracing | MIT | Standard |
| `thiserror` | Typed errors | MIT/Apache-2.0 | Standard |
| `uuid` (v7), `time` or `chrono` | IDs, timestamps | MIT/Apache-2.0 | Time-ordered IDs |
| `sha2`, `ed25519-dalek` | Audit hash chain, manifest signatures | MIT/Apache-2.0, BSD-3 | RustCrypto / dalek, widely audited |
| `jsonschema` | Tool/agent I/O contract validation | MIT | Mature JSON Schema validator |
| `clap` | CLI | MIT/Apache-2.0 | Standard |
| `secrecy` | Secret wrapper | MIT/Apache-2.0 | Prevents accidental exposure |
| `semver` | Version compatibility | MIT/Apache-2.0 | Standard |
| `nix` or `rustix` | rlimits, process groups | MIT | Unix process control |
| dev: `tempfile`, `assert_cmd`, `proptest` | Tests | MIT/Apache-2.0 | Standard |

Exact versions will be pinned to releases compatible with rustc 1.86 (or a newer toolchain if D-06 is approved), recorded in `Cargo.lock`, and listed in ADR-0003.

### 15.2 Implementation sequence (compile + test gate after every step)

| Step | Deliverable | Gate |
|---|---|---|
| 0 | Baseline: workspace skeleton, `rust-toolchain.toml`, lint config, `scripts/ci.sh`, docs skeleton (`docs/architecture/`, ADR-0001…0006, ROADMAP, ACTION-ITEMS, TEST-STRATEGY) | `cargo build`, fitness test scaffold |
| 1 | `eaeh-core`: IDs, errors, `Principal`, scopes, `Secret<T>`, clock, config | unit tests |
| 2 | `eaeh-store`: SQLite pool, migrations, repositories, hash-chained audit log | migration + tamper-detection tests |
| 3 | `eaeh-events`: envelope, bus, outbox, dedup, retry, dead-letter | duplicate-delivery and DLQ tests |
| 4 | `eaeh-governance`: authorizer (RBAC + scope), policy engine, approvals (SoD, no self-approval), budgets | security unit tests |
| 5 | `eaeh-extensibility`: hooks, middleware, interceptors, plugin manifests/registry/integrity | hook failure-policy tests, signature tests |
| 6 | `eaeh-isolation`: provider trait, restricted process provider, container provider | real escape-attempt tests |
| 7 | `eaeh-workspace`: workspaces, path confinement, locks, artifacts + provenance | traversal/symlink/concurrency tests |
| 8 | `eaeh-tools`: registry, gateway pipeline, built-in tools | unauthorized-tool, budget, hook-bypass tests |
| 9 | `eaeh-intelligence`: model abstraction, registry, router, fallback, prompts, context assembly | injection-precedence, fallback tests |
| 10 | `eaeh-validation`: validator contracts, schema validator, quality gate | unit tests |
| 11 | `eaeh-agents`: definitions, registry, discovery, lifecycle, messaging, delegation limits | delegation cycle/depth tests |
| 12 | `eaeh-workflow`: definitions, engine, approval gates, planner trait, checkpoints, recovery | approval-bypass, recovery tests |
| 13 | `eaeh-runtime`: composition root, supervision, correction bounds, observability wiring | integration tests |
| 14 | `eaeh-api` + `eaeh-cli` | API contract tests, CLI tests |
| 15 | E2E, security, fitness suites; threat model; documentation | full suite |
| 16 | Completion procedure (§40), `PHASE-01-COMPLETION.md`, roadmap/action register, Phase 2 plan | exit decision |

## 16. Phase 1 Acceptance Criteria and Test Scenarios

| ID | Acceptance criterion | Verification |
|---|---|---|
| AC-01 | Workspace builds; `fmt`, `clippy -D warnings`, and all tests pass via `scripts/ci.sh` | CI script output |
| AC-02 | Crate dependency graph follows §6.1; no cycles; governance independent of intelligence | Fitness test |
| AC-03 | Workflow state is durable; a process interrupted mid-workflow resumes from last checkpoint without duplicate side effects | Recovery E2E test |
| AC-04 | Hybrid orchestration: planner proposals that violate workflow definition or policy are rejected by the engine | Integration test |
| AC-05 | Agents are registered with versioned definitions, discovered by capability, instantiated and terminated dynamically; lifecycle hooks fire | Integration test |
| AC-06 | Agent delegation enforces authorization, max depth, cycle and fan-out limits | Security test |
| AC-07 | Isolation provider abstraction with restricted-process provider; timeouts, env clearing, cwd confinement verified by real escape attempts | Isolation tests |
| AC-08 | Container provider enforces no-network, read-only rootfs, memory/pids limits – verified by real escape attempts | Isolation tests (**BLOCKED if no container runtime available**) |
| AC-09 | Tools requiring isolation capabilities a provider cannot enforce are refused | Gateway test |
| AC-10 | An agent cannot invoke an unauthorized tool or a tool above its autonomy level | Security test |
| AC-11 | Approval gates cannot be bypassed via direct API calls; agents cannot self-approve; SoD enforced; rejection returns to revision with history | API + security tests |
| AC-12 | Policy engine is deterministic, default-deny, deny-overrides; decisions recorded | Unit + audit tests |
| AC-13 | Hook framework supports registration, priority ordering, filters, timeouts, enable/disable; failure follows block/warn/continue | Unit tests |
| AC-14 | Mandatory controls remain enforced when hooks are missing, disabled or failing | Security test |
| AC-15 | Middleware and interceptors have distinct contracts, deterministic order and error propagation | Unit tests |
| AC-16 | Typed versioned events with correlation/causation IDs; duplicate events do not cause duplicate side effects; failing consumers reach dead-letter after bounded retries | Event tests |
| AC-17 | Plugin manifests validated; tampered digest / invalid signature rejected; plugins cannot bypass gateway or policy | Security test |
| AC-18 | Execution budgets (tool calls, tokens, cost, duration, delegation depth, correction attempts) enforced; supervision detects stalled tasks and repeated failures and escalates | Integration tests |
| AC-19 | Context assembly respects trust precedence; untrusted tool output/retrieved content cannot alter system instructions | Security test |
| AC-20 | Prompts are versioned assets with validation and rollback | Unit tests |
| AC-21 | LLM provider abstraction with routing by capability/cost and fallback on provider failure; model/provider version and usage recorded | Integration tests (scripted provider) |
| AC-22 | Validation framework contracts with severity, evidence and quality-gate aggregation | Unit tests |
| AC-23 | Workspaces confine file access (traversal/symlink rejected); concurrent modifications controlled by locks; artifacts content-addressed with provenance | Security + concurrency tests |
| AC-24 | Cross-project/tenant access denied at authz and store-query level | Security test |
| AC-25 | Secrets never appear in logs, audit payloads or agent context | Log-capture test |
| AC-26 | Audit log is hash-chained; tampering is detected by `eaeh audit verify` | Integration test |
| AC-27 | Structured logs/traces carry correlation IDs; health and metrics endpoints respond | API tests |
| AC-28 | CLI and `/v1` API: authenticated, versioned, consistent error model, pagination on list endpoints | API contract + CLI tests |
| AC-29 | Documentation: architecture docs, ADRs, ROADMAP, ACTION-ITEMS, TEST-STRATEGY, THREAT-MODEL, completion report | Document review |

## 17. Risks, Dependencies and Unresolved Decisions

### 17.1 Risks

| ID | Risk | Impact | Mitigation |
|---|---|---|---|
| R-01 | Phase 1 scope is very broad (22 foundation items) | Shallow or placeholder implementations | Implement each as a minimal *real* primitive; report depth honestly; defer non-foundation features explicitly |
| R-02 | rustc 1.86 may be below MSRV of current crate releases | Build failures, older dependency versions | Pin compatible versions, or upgrade toolchain (D-06) |
| R-03 | macOS cannot enforce network/memory isolation for plain processes | Weak isolation if only process provider used | Capability declaration + refusal; container provider for strong isolation |
| R-04 | Docker daemon not running; Podman machine state unknown | Container isolation unverifiable (AC-08 BLOCKED) | Start Docker Desktop or `podman machine start` before step 6 |
| R-05 | No real LLM provider in Phase 1 | Agent behaviour validated only with scripted provider | Clearly label; real adapter as Phase 2 early item or optional Phase 1 add-on (D-04) |
| R-06 | Single human in the loop | SoD tests need multiple identities | Tests create multiple principals; production SoD is configuration-driven |
| R-07 | No enterprise standards provided | Policies could be "invented" | Ship only structural/default-deny policies; enterprise policies are configuration supplied by the organization |
| R-08 | `cargo-deny`/`cargo-audit` not installed | Supply-chain gate incomplete | Install (A-07) or record as accepted exception |
| R-09 | SQLite single-writer limits | Throughput under concurrent workflows | Acceptable for Phase 1; repository traits allow Postgres later (Phase 8) |

### 17.2 Unresolved decisions (require your approval)

| ID | Decision | Recommendation |
|---|---|---|
| D-01 | Persistence technology | SQLite via SQLx with repository traits (no external DB server) |
| D-02 | Policy engine | In-house deterministic rule engine behind `PolicyEngine` trait; evaluate Cedar adapter later |
| D-03 | Plugin execution hosts in Phase 1 | In-process host only; WASM & external-process: manifest/registry/validation only, hosts deferred (not reported as implemented) |
| D-04 | Real LLM provider adapter | Not in Phase 1; abstraction + test-only scripted provider |
| D-05 | OpenTelemetry export | Optional cargo feature, not a Phase 1 acceptance criterion |
| D-06 | Rust toolchain | Pin 1.86 now (works offline); upgrading to current stable is welcome if you prefer |
| D-07 | Container runtime for isolation tests | Docker or Podman, whichever you will have running |

### 17.3 Action items raised by this assessment

| ID | Description | Priority | Dependency | Role | Target phase | Status |
|---|---|---|---|---|---|---|
| A-01 | Approve architecture proposal and Phase 1 scope | Critical | – | Product owner / Architect | 1 | Open |
| A-02 | Resolve decisions D-01…D-07 | High | A-01 | Architect | 1 | Open |
| A-03 | Make a container runtime available (Docker daemon or Podman machine) | High | – | Platform engineer | 1 | Open |
| A-04 | Create ADRs for approved decisions and D-01…D-07 | High | A-02 | Architect | 1 | Open |
| A-05 | Create ROADMAP, ACTION-ITEMS, TEST-STRATEGY, architecture docs | High | A-01 | Engineering lead | 1 | Open |
| A-06 | Expand README with project purpose, build and usage | Medium | Step 14 | Engineering lead | 1 | Open |
| A-07 | Install `cargo-deny` (licences/advisories) or accept exception | Medium | – | Platform engineer | 1 | Open |
| A-08 | Decide CI hosting (e.g. GitHub Actions) – Phase 1 provides `scripts/ci.sh` only | Low | – | Platform engineer | 1/6 | Open |
| A-09 | Supply enterprise policies/standards (not to be invented) | Medium | – | Enterprise architecture | 2 | Open |

---

**Assessment conclusion:** The repository is greenfield. Nothing can be reused except the licence and the master instructions. The proposed architecture follows all approved decisions in §3 without deviation. Implementation will not begin until this assessment and the decisions in §17.2 are approved.
