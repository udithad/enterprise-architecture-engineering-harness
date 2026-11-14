# PHASE-01 – Completion Report: Core Harness Foundation

| Field | Value |
|---|---|
| Phase | 1 – Core Harness Foundation |
| Report date | 2026-11-12 |
| Base commit | `1527c09` (all Phase 1 work is **uncommitted** in the working tree — see A-27) |
| Toolchain | rustc / cargo 1.99.1 (pinned, `rust-toolchain.toml`) |
| Host of record | macOS 26.7.1 (Darwin 25.6.0) arm64; Podman 5.5.1 (`podman-machine-default`, applehv) |
| Evidence | `docs/implementation/evidence/phase-01-ci.log` (full `scripts/ci.sh` output), `docs/implementation/evidence/phase-01-api-smoke.log` (`scripts/smoke-api.sh` against the real server) |
| Proposed exit decision | **READY WITH ACCEPTED EXCEPTIONS** — contingent on acceptance of exception E-1 (§14) |

---

## 1. Executive summary

Phase 1 delivers a working, governed core harness: a 16-crate Rust workspace producing one
binary (`eaeh`) with a `/v1` HTTP API and CLI. Deterministic components (workflow engine,
policy engine, authorization, human approvals, budgets) are authoritative; dynamically
instantiated agents propose work and act only through a governed tool gateway and governed
model client; tool execution that runs code is confined to a container provider whose
isolation claims are verified by real escape-attempt tests.

Verification: `scripts/ci.sh` passed — `cargo fmt --check`, `cargo clippy --all-targets
-D warnings`, build, **140 automated tests: 140 passed, 0 failed, 0 ignored**, and
`cargo deny check` (advisories, bans, licences, sources all ok, with one documented
advisory exception). The real-server smoke script passed. 28 of 29 acceptance criteria are
IMPLEMENTED AND VERIFIED; AC-08 is verified for Podman only (Docker not exercised).

Four defects were found during the phase by the test suites and fixed (§11). One
supply-chain advisory (RUSTSEC-2026-0459, bundled SQLite) cannot be fixed until sqlx
releases support for the patched `libsqlite3-sys`; it is mitigated, documented and requires
your explicit acceptance.

Out of scope and **not** implemented, as instructed: knowledge/skills management, BRD
analysis, HLD/LLD generation, development/test/Git/CI/CD/release workflows, a production
LLM provider (D-04), WASM/external-process plugin hosts (D-03), OpenTelemetry export (D-05).

## 2. Expected versus actual deliverables (master §28)

| # | Phase 1 scope item | Status | Where / notes |
|---|---|---|---|
| 1 | Rust core runtime | Delivered | `eaeh-core`, `eaeh-runtime::harness` |
| 2 | Hybrid orchestration foundation | Delivered | `eaeh-workflow` (authoritative) + `Planner`/`TaskExecutor` implemented in `eaeh-runtime::executor` |
| 3 | Durable workflow state foundation | Delivered | instances, transitions with checkpoints, idempotent tasks, recovery |
| 4 | Agent registry and definitions | Delivered | `eaeh-agents::{definition, registry}`; 4 shipped definitions |
| 5 | Agent discovery and dynamic lifecycle | Delivered | capability discovery (smallest effective), instantiate/execute/terminate |
| 6 | Multi-agent communication foundation | Delivered (foundation) | governed delegation + persisted typed messages; no model-initiated delegation tool yet |
| 7 | Isolated execution provider abstraction + initial implementation | Delivered | restricted-process and container providers; declared capabilities |
| 8 | Basic policy and approval framework | Delivered | `RuleEngine`, `ApprovalManager`, gates, tool approvals |
| 9 | Hook framework and core lifecycle hooks | Delivered | 42 hook points; emitters for agent, task/workflow, LLM, tool, context, artifact/file points; build/test/commit/PR/deploy/knowledge points are contracts only |
| 10 | Middleware and interceptor foundation | Delivered | `Pipeline`, `InterceptorChain`; redaction + size interceptors on tool output |
| 11 | Typed event model and core event delivery | Delivered | catalogue v1, transactional outbox, dispatcher with retry/dead-letter/ordering |
| 12 | Plugin/extension contracts and registry foundation | Delivered (in-process only) | manifests, digest + ed25519, admin enablement, grants, lifecycle |
| 13 | Tool registry and secure basic tool gateway | Delivered | 14-step governed pipeline; `fs.read/list/write`, `exec.run` |
| 14 | Execution budgets and basic supervision | Delivered | durable ledger; stall/repeated-failure escalation; loop detection |
| 15 | Context and prompt management foundations | Delivered | trust-ordered assembler with snapshots; versioned prompts |
| 16 | LLM provider abstraction | Delivered | registry, routing, fallback, circuit breaker, usage recording; **no production provider** (D-04) |
| 17 | Core validation framework contracts | Delivered | schema contracts, validators, quality gates |
| 18 | Basic workspace and artifact management | Delivered | confined workspaces, locks, content-addressed versioned artifacts |
| 19 | Persistence and recovery | Delivered | SQLite/SQLx, migrations, serialized IMMEDIATE writer, recovery |
| 20 | Basic audit and observability | Delivered | hash-chained audit + verify; redacting JSON logs with correlation spans; metrics; health |
| 21 | CLI/API foundation | Delivered | `/v1` API (23 routes, 26 method/route combinations), `eaeh` CLI |
| 22 | Automated test infrastructure | Delivered | 140 tests, fitness suite, `scripts/ci.sh`, `scripts/smoke-api.sh`, `cargo-deny` |

## 3. Architecture and implementation summary

Modular monolith, five logical layers mapped onto crates (ADR-0002). Dependency direction is
enforced by `tests/tests/fitness.rs` from `cargo metadata`:

```
core ← store ← events ← governance ← extensibility ← {workspace, intelligence, tools} ← agents
core ← {isolation, validation};  {governance, extensibility, workspace} ← workflow
all ← runtime (composition root) ← api ← cli;   testkit = dev-only
```

Architectural documents: `docs/architecture/README.md` (overview) and topic documents;
ADR-0001…0011 record the approved decisions D-01…D-07 and clarifications 1–6.

## 4. Component responsibilities and data flows

Full catalogue (26 master components with crate, status, interfaces, security boundary):
`docs/architecture/components.md`. Principal flows:

1. **Tool invocation** (`ToolGateway::invoke`): version resolution → agent allow-list →
   authorization → autonomy ceiling → input schema → policy → approval binding → workspace
   scope + isolation selection (refuse, never downgrade) → rate limit → budget → hooks →
   middleware → execution (timeout, idempotent retry) → output schema → interceptors
   (redaction, size) → hooks → audit + `tool.invoked` in one transaction → `Untrusted`
   output. (`docs/architecture/agents-and-tools.md`)
2. **Workflow** (`WorkflowEngine::drive`): gate-prerequisite check → idempotent task →
   `TaskExecutor` (agent or deterministic tool step) → artifact with provenance → approval
   gate bound to artifact hash → human decision in `ApprovalManager` → transition with
   checkpoint, audit, event. Recovery resumes interrupted runs without repeating completed
   steps. (`docs/architecture/runtime-and-orchestration.md`)
3. **Hybrid planning**: planner agent → `Untrusted<PlanProposal>` → engine validates kinds,
   step count, per-step policy → executes steps → audit `plan.accepted|rejected`.
4. **Agent execution**: role-less, project-bound agent principal → trust-layered context
   (system > agent instruction > task input > untrusted data, fenced) → governed model call
   → proposed tool calls through the gateway → schema-validated output → termination.

## 5. Implementation inventory

| Crate | Lines (src + tests) | Responsibility |
|---|---|---|
| `eaeh-core` | 1,154 | ids, errors, principals, scopes, risk/autonomy, secrets/redaction, clock, trace, `Untrusted` |
| `eaeh-store` | 748 | SQLite pool, migrations, serialized writer, hash-chained audit, idempotency |
| `eaeh-events` | 705 | envelopes, catalogue, outbox, dispatcher |
| `eaeh-governance` | 2,432 | identity & tokens, RBAC/scope authorization, policy engine, approvals, budgets |
| `eaeh-extensibility` | 1,737 | hooks, middleware, interceptors, plugins |
| `eaeh-isolation` | 1,045 | provider abstraction, restricted process, container provider + 13 isolation tests |
| `eaeh-workspace` | 980 | workspaces, locks, artifacts |
| `eaeh-validation` | 539 | schema contracts, validators, quality gates |
| `eaeh-tools` | 1,402 | registry, gateway, built-in tools + 10 gateway tests |
| `eaeh-intelligence` | 1,751 | models, client, prompts, context |
| `eaeh-agents` | 1,779 | definitions, registry, runtime, delegation |
| `eaeh-workflow` | 2,633 | definitions, engine |
| `eaeh-runtime` | 1,876 | config, harness, executors, supervision, observability, extension host |
| `eaeh-api` | 1,135 | HTTP API |
| `eaeh-cli` | 871 | CLI binary |
| `eaeh-testkit` | 141 | **test double** `ScriptedModelProvider` (dev-only) |
| `tests/` (`eaeh-acceptance`) | 891 | fitness (8), e2e (3), logging (1) suites |

Totals: 21,819 lines of Rust (17,419 in `src/` including in-file unit tests; 4,399 in test
files). 296 packages in `Cargo.lock` (16 internal + acceptance crate; remainder third-party).
Configuration and assets: `config/eaeh.toml`, `config/agents/*.json` (4),
`config/policies/` (empty by design), `prompts/*.toml` (3), `workflows/*.json` (2),
`crates/eaeh-store/migrations/0001_initial.sql` (26 tables), `deny.toml`,
`rust-toolchain.toml`, `scripts/ci.sh`, `scripts/smoke-api.sh`.

## 6. Acceptance criteria and statuses

| ID | Criterion | Status | Evidence (tests in `phase-01-ci.log`) |
|---|---|---|---|
| AC-01 | Workspace builds; fmt, clippy `-D warnings`, all tests pass via `scripts/ci.sh` | IMPLEMENTED AND VERIFIED | `phase-01-ci.log` ends `==> CI PASSED`; 140/140 |
| AC-02 | Dependency graph follows layering; no cycles; governance independent of intelligence | IMPLEMENTED AND VERIFIED | `fitness.rs` (8 tests) |
| AC-03 | Durable state; interrupted workflow resumes without duplicate side effects | IMPLEMENTED AND VERIFIED | `engine.rs::interrupted_workflow_recovers_without_duplicate_side_effects` |
| AC-04 | Planner proposals violating definition/policy rejected | IMPLEMENTED AND VERIFIED | `engine.rs::hybrid_orchestration_validates_planner_proposals`; `e2e.rs::agentic_workflow_with_planning_gate_and_full_traceability` |
| AC-05 | Versioned agents registered, discovered, dynamically instantiated/terminated; lifecycle hooks fire | IMPLEMENTED AND VERIFIED | `agents.rs::registration_rules_and_smallest_effective_discovery`, `::dynamic_lifecycle_with_tools_hooks_and_events` |
| AC-06 | Delegation enforces authorization, depth, cycle, fan-out | IMPLEMENTED AND VERIFIED | `agents.rs::delegation_controls`, `::depth_limit_enforced` |
| AC-07 | Restricted-process provider: timeout, env, confinement claims verified | IMPLEMENTED AND VERIFIED | `process_isolation.rs` (6) — note: it declares fs/network/memory/pids/privilege **not enforced** |
| AC-08 | Container provider: no network, read-only rootfs, memory/pids limits verified by escapes | IMPLEMENTED AND VERIFIED (Podman 5.5.1) | `container_isolation.rs` (7); Docker not exercised (E-2) |
| AC-09 | Tools needing unavailable isolation are refused | IMPLEMENTED AND VERIFIED | `gateway.rs::isolation_requirements_refused_when_unavailable`, `isolation::tests::selection_refuses_instead_of_downgrading` |
| AC-10 | Agent cannot invoke unauthorized tool or exceed autonomy | IMPLEMENTED AND VERIFIED | `gateway.rs::autonomy_and_allowlist_enforced`, `agents.rs::prompt_injection_in_tool_output_cannot_escalate` |
| AC-11 | No approval bypass via API; no self-approval; SoD; rejection returns to revision with history | IMPLEMENTED AND VERIFIED | `api.rs::approval_gate_cannot_be_bypassed_through_the_api`, `engine.rs::gate_*`, `::rejection_returns_to_revision_with_history`, `approval::tests::*`, `cli.rs::cli_end_to_end` |
| AC-12 | Policy engine deterministic, default-deny, deny-overrides; decisions recorded | IMPLEMENTED AND VERIFIED | `policy::tests::*` (4); matched rules recorded in tool/plan audit records (`policy.evaluated` event not emitted — TD-04) |
| AC-13 | Hooks: registration, ordering, filters, timeouts, enable/disable, failure policies | IMPLEMENTED AND VERIFIED | `hooks::tests::*` (4) |
| AC-14 | Mandatory controls hold when hooks are missing/disabled/failing | IMPLEMENTED AND VERIFIED | `gateway.rs::mandatory_controls_hold_when_hooks_missing_disabled_or_failing` |
| AC-15 | Middleware & interceptors: distinct contracts, deterministic order, error propagation | IMPLEMENTED AND VERIFIED | `middleware::tests::*` (3), `interceptor::tests::*` |
| AC-16 | Typed versioned events with correlation/causation; duplicates harmless; dead-letter | IMPLEMENTED AND VERIFIED | `dispatcher::tests::*` (5), `e2e.rs::agentic_…` (re-dispatch) |
| AC-17 | Plugin manifests validated; tampered digest/invalid signature rejected; no gateway/policy bypass | IMPLEMENTED AND VERIFIED (in-process only) | `plugin::tests::*`, `harness.rs::plugins_require_admin_enablement_grants_and_cannot_bypass_policy`; WASM/external hosts NOT IMPLEMENTED by decision D-03 |
| AC-18 | Budgets (tool calls, tokens, cost, duration, delegation depth, corrections) enforced; stalled & repeated failures escalated | IMPLEMENTED AND VERIFIED | `budget::tests::*`, `gateway.rs::budgets_enforced_and_fail_closed`, `model_client.rs::all_failing_and_budget_limits`, `::cost_budget_is_enforced_independently_of_tokens`, `agents.rs::bounded_correction_loop_detection_and_step_budget`, `::depth_limit_enforced`, `engine.rs::retry_and_timeout` (duration), `harness.rs::supervision_escalates_stalled_work`, `::supervision_escalates_repeated_failures`. Note TD-03: duration and delegation depth are enforced by timeouts/runtime configuration, not by the ledger resources of the same name |
| AC-19 | Trust precedence; untrusted content cannot alter system instructions | IMPLEMENTED AND VERIFIED | `context::tests::*`, `agents.rs::prompt_injection_in_tool_output_cannot_escalate` |
| AC-20 | Prompts versioned with validation and rollback | IMPLEMENTED AND VERIFIED | `prompt::tests::*` |
| AC-21 | Model routing by capability/cost; fallback; provider/model version and usage recorded | IMPLEMENTED AND VERIFIED (with test doubles) | `model_client.rs` (5), `e2e.rs::agentic_…` (model versions in provenance) |
| AC-22 | Validation contracts with severity, evidence, quality gates | IMPLEMENTED AND VERIFIED | `eaeh-validation` tests (4) |
| AC-23 | Workspace confinement; concurrency control; content-addressed artifacts with provenance | IMPLEMENTED AND VERIFIED | `workspace::tests::*` (5), `artifacts::tests::*`, `e2e.rs::agentic_…` (provenance) |
| AC-24 | Cross-project/tenant access denied at authz and store level | IMPLEMENTED AND VERIFIED | `authz::tests::cross_tenant_and_cross_project_denied`, `gateway.rs::cross_project_access_denied`, `api.rs::cross_project_isolation_and_role_checks`, `workspace::tests::scope_mismatch_and_cleanup`, `artifacts::tests::*` |
| AC-25 | Secrets never in logs, audit payloads or agent context | IMPLEMENTED AND VERIFIED | `logging.rs::logs_are_redacted_and_carry_correlation_ids`, `e2e.rs::secrets_never_reach_audit_events_or_agent_context`, `audit::tests::details_are_redacted_before_storage`, smoke log (0 token literals). Residual: unregistered secrets that match no pattern (A-15) |
| AC-26 | Hash-chained audit; tampering detected by `eaeh audit verify` | IMPLEMENTED AND VERIFIED | `audit::tests::*`, `cli.rs::cli_end_to_end`, smoke (`valid=True records=32`) |
| AC-27 | Structured logs carry correlation ids; health and metrics respond | IMPLEMENTED AND VERIFIED | `logging.rs::logs_are_redacted_and_carry_correlation_ids`, `api.rs::metrics_and_health_details_require_permission`, `harness.rs::shipped_definitions_and_demo_workflow_end_to_end`, smoke |
| AC-28 | CLI and `/v1` API: authenticated, versioned, consistent errors, pagination | IMPLEMENTED AND VERIFIED | `api.rs` (5), `cli.rs` (1), smoke |
| AC-29 | Documentation set complete | IMPLEMENTED AND VERIFIED (by document inspection) | §9 list; `docs/` |

Counts: 29 criteria — 29 IMPLEMENTED AND VERIFIED (AC-08 with the Podman-only qualification);
0 IMPLEMENTED BUT NOT VERIFIED; 0 PARTIALLY IMPLEMENTED; 0 NOT IMPLEMENTED; 0 BLOCKED.
Capabilities intentionally not implemented by approved decision (not criteria failures):
production model provider (D-04), WASM/external plugin hosts (D-03), OTel export (D-05).

## 7. Step-by-step testing instructions

Prerequisites: rustup; Podman (or Docker) running; `podman pull docker.io/library/alpine:3.20`;
optional `cargo install --locked cargo-deny`; `curl` and `python3` for the smoke script.

| Step | Command (from repository root) | Expected output |
|---|---|---|
| 1 | `scripts/ci.sh` | sections `==> fmt`, `==> clippy`, `==> build`, `==> test` (every suite `test result: ok`), `==> cargo-deny` (`advisories ok, bans ok, licenses ok, sources ok`), final line `==> CI PASSED` |
| 2 | `cargo test --workspace --locked 2>&1 \| grep '^test result' \| awk '{p+=$4; f+=$6} END {print p, f}'` | `140 0` |
| 3 | `cargo test -p eaeh-isolation --test container_isolation` | `7 passed`; fails (not skips) if no container runtime |
| 4 | `cargo test -p eaeh-acceptance --test fitness` | `8 passed` |
| 5 | `cargo test -p eaeh-acceptance` | e2e 3, fitness 8, logging 1 passed |
| 6 | `scripts/smoke-api.sh` | see §8 transcript; final line `== SMOKE PASSED` |
| 7 | Manual: `target/debug/eaeh init`, `admin bootstrap …`, `serve`, then the curl calls in `scripts/smoke-api.sh` | as in §8 |

Container suites without a runtime: `EAEH_SKIP_CONTAINER_TESTS=1 scripts/ci.sh` prints
`SKIPPED` and must then be reported as not verified.

## 8. Actual test results and evidence

* `phase-01-ci.log` (2026-11-12, macOS arm64, Podman 5.5.1, rustc 1.99.1): exit 0;
  **140 passed, 0 failed, 0 ignored** across 29 test binaries; clippy clean; cargo-deny
  `advisories ok, bans ok, licenses ok, sources ok` (RUSTSEC-2026-0459 ignored per E-1).
* Stability: the full suite was run three additional consecutive times before the evidence
  run: 140/140 each time.
* `phase-01-api-smoke.log` (real `eaeh serve`, container provider available) — actual
  output:

```
== init                              ok [1]
== bootstrap                         token file mode: 600
== health (unauthenticated)          {"api_version":"v1","status":"ok"}
== /v1/me without token -> 401
== start demo.approval as operator   status=waiting_approval state=review
== operator tries to approve -> 403
== approver with wrong artifact hash -> 409
== approver approves                 approval=approved by arthur
                                     status=succeeded path=draft->review review->publish publish->done
== audit verify (admin)              valid=True records=32
== audit as operator -> 403
== metrics                           eaeh_events_total{type="workflow.completed"} 1 / eaeh_outbox_pending 0
== isolation providers               [['container', 'available'], ['restricted-process', 'available']]
== token literals in server log: 0
== SMOKE PASSED
```

(Layout condensed; the log file contains the verbatim output.)

## 9. Installation, configuration, startup, shutdown and usage

Full guide with CLI and API reference: `docs/OPERATIONS.md`. Summary:

```bash
cargo build --workspace --locked
B=target/debug/eaeh
$B init                                                    # data dir 0700, DB 0600, migrations, definitions
$B admin bootstrap --tenant acme --project platform --admin-id admin --token-file ./admin.token
export EAEH_TOKEN="$(cat admin.token)"
$B principal create --id arthur --display-name Arthur --roles approver --projects <project-id>
$B token issue --principal arthur --label laptop
$B serve                                                   # Ctrl-C for graceful shutdown
$B workflow start --definition demo.approval --project <project-id>
$B approval list --project <project-id> --status pending
$B approval decide <approval-id> --decision approve --artifact-hash <hash>   # as an approver
$B audit verify
```

Configuration: `config/eaeh.toml` (unknown keys rejected; no credentials; secrets via
`EAEH_SECRET_*`). Documentation produced: `README.md`; `docs/OPERATIONS.md`;
`docs/architecture/{README, components, runtime-and-orchestration, agents-and-tools,
extensibility, isolation, intelligence, security, data, integrations}.md`;
`docs/architecture/security/THREAT-MODEL.md`; `docs/architecture/adr/ADR-0001…0011`;
`docs/implementation/{ROADMAP, ACTION-ITEMS, TEST-STRATEGY}.md`; this report; evidence logs.

## 10. Security and governance assessment

Threat model with 27 threats, mitigations, evidence and residual risk:
`docs/architecture/security/THREAT-MODEL.md`. Assessment against the master instructions:

| Requirement | Assessment |
|---|---|
| LLM never authorizes mandatory operations | Met — model output is `Untrusted`; every action passes the gateway/engine; agents cannot hold roles or decide approvals |
| Approvals enforced at workflow and API boundaries | Met — inside `ApprovalManager` and engine gate prerequisite check; API/CLI tests |
| Controls independent of hooks | Met — tested with missing, disabled and failing hooks |
| Isolation claims tested | Met for Podman; process provider declares its limits |
| Plugins cannot bypass controls | Met for contributed tools/hooks; **in-process plugins are trusted code** (clarification 1) |
| Secrets protected | Met for registered secrets and token patterns; no encryption at rest (A-15) |
| No default credentials; provisioning, rotation, revocation | Met (ADR-0011, CLI and API tests) |
| Tenant/project isolation | Met at authz, store, context, workspace, artifact levels |
| Audit and traceability | Met — hash chain; one correlation id reconstructs a run (`e2e.rs::agentic_…` asserts 10 audited action types under the workflow correlation); artifact provenance names agent, model version, prompts, tools, context snapshot |
| Supply chain | Gate in CI; exception E-1 pending acceptance; SBOM not automated (A-16) |
| Separation of governance from intelligent orchestration | Met and fitness-enforced (stricter than proposed — see D-1 in §11) |

## 11. Known limitations, defects, risks and technical debt

### Defects found and fixed during Phase 1 (all re-tested)

| ID | Defect | Found by | Fix |
|---|---|---|---|
| F-1 | Podman injected host proxy variables (`NO_PROXY`) into containers (environment leak) | `container_isolation.rs::environment_contains_only_explicit_variables` | `--http-proxy=false` for Podman; Docker limitation documented (ISO-L2) |
| F-2 | Approval decisions audited under the approver's request correlation, breaking run traceability | `e2e.rs::agentic_workflow_with_planning_gate_and_full_traceability` | decisions recorded under the workflow's correlation (request id kept as causation) |
| F-3 | Intermittent `SQLITE_BUSY_SNAPSHOT` (code 517) when background event dispatch committed between a transaction's read and write; three writes bypassed the serialized writer | e2e suite (3 of 6 runs failed before the fix) | all writes through `Db::write()`; `BEGIN IMMEDIATE`; new fitness test `all_database_writes_use_the_serialized_writer`; 8/8 and 3×140/140 stable afterwards |
| F-4 | Oversized request bodies returned 400 instead of 413 | `api.rs::authentication_and_error_contract` | rejection status preserved |

Test-only issues fixed (not product defects): a policy test case reused a denied resource;
a log-capture test used a thread-scoped subscriber that is racy under parallel tests (moved
to its own process, `tests/tests/logging.rs`); a shell-quoting error in the smoke script.

### Deviations from the approved assessment (for acknowledgement)

| ID | Deviation | Rationale / impact |
|---|---|---|
| D-1 | `eaeh-workflow` does **not** depend on `eaeh-agents` (assessment §6.1 placed workflow above agents). Agents are reached through `TaskExecutor`/`Planner` traits implemented in `eaeh-runtime`. | Stricter separation of deterministic orchestration from intelligent orchestration (clarification 5); no functional loss |
| D-2 | `eaeh-validation` was built before `eaeh-tools` (sequence steps 8/10 swapped). | The gateway needs schema contracts; same layer |
| D-3 | Workspace-level test crate is named `eaeh-acceptance` and lives in `tests/` (assessment named `tests/e2e`, `tests/security`, `tests/fitness`). | Single crate with three test binaries (`e2e`, `fitness`, `logging`); security tests are distributed across crates and mapped in TEST-STRATEGY |
| D-4 | `cargo-deny` licence allow-list includes `MIT-0`. | OSI-approved, more permissive than MIT; transitive via jsonschema |

### Known limitations

* No production LLM provider (D-04): agentic workflows (`engineering.task`) return
  `unavailable` in the shipped binary; only `demo.approval` runs without a model.
* In-process plugins are trusted code; WASM/external hosts deferred (A-12).
* Container isolation verified on Podman/macOS only; Docker and Linux hosts not exercised
  (A-26). Docker proxy-variable injection cannot be disabled per run (ISO-L2).
* The restricted-process provider is registered but no shipped tool selects it (file tools
  run in-process; `exec.run` requires the container provider).
* Single-process registries for hooks/tools/plugins and an in-memory API rate limiter;
  plugin enablement takes effect on restart.
* Audit chain detects modification but not a consistent rewrite of the whole chain by an
  attacker with database write access (A-18).
* No encryption at rest, retention or data classification (A-15); no SBOM (A-16).
* Multi-process writers (CLI and server on the same database) are designed for but not
  stress-tested (A-20).
* Token estimation is a heuristic (≈4 chars/token) until a provider reports real usage.

### Risks

| ID | Risk | Status |
|---|---|---|
| R-01 | Breadth of Phase 1 led to shallow primitives | Mitigated: every item has tested, real behaviour; depth limits listed above |
| R-02 | Toolchain/MSRV | Closed: 1.99.1 pinned |
| R-03 | macOS process isolation weakness | Mitigated by declared capabilities and refusal |
| R-04 | Container runtime availability | Closed for this host (Podman running); open for CI (A-08/A-26) |
| R-05 | Agents validated only with scripted provider | Open until A-11 (Phase 2) and Phase 5 evaluation |
| R-07 | Inventing enterprise standards | Avoided: only structural baseline policy ships (A-09) |
| R-10 | **New:** upstream sqlx caps patched SQLite (E-1) | Open — depends on sqlx release |

### Technical debt

| ID | Item | Action |
|---|---|---|
| TD-01 | SQLite-specific SQL in domain crates; no per-repository storage traits | A-24 |
| TD-02 | Heuristic token estimation | Phase 2 with A-11 |
| TD-03 | `wall_clock_ms` and `delegation_depth` ledger resources declared but enforcement is via timeouts / runtime config | A-23 |
| TD-04 | `task.assigned`, `policy.evaluated`, `budget.exhausted` events defined but not emitted | A-22 |
| TD-05 | No HTTP/CLI routes to register prompts, policies, agent or workflow definitions (loaded from files at startup; services exist) | A-21 |
| TD-06 | Task heartbeats only on status changes; `stall_after_secs` must exceed the longest state timeout | A-25 |
| TD-07 | In-process registries/rate limiter not shared across instances | Phase 8 |
| TD-08 | Plugin capabilities `register_middleware`, `register_interceptors`, `register_validators` are declarable but not exposed by `ExtensionContext` | A-28 |

## 12. Next-phase scope, dependencies and acceptance criteria

Recommended Phase 2 (Enterprise Knowledge and Skills Management) scope, entry criteria and
acceptance criteria: `docs/implementation/ROADMAP.md` §"Phase 2". Key dependencies:
decision on E-1/A-10, selection of the first production model provider (A-11, needed for
embeddings and real agent runs), enterprise standards source (A-09), CI hosting (A-08).
**Phase 2 has not been started.**

## 13. Action register

Current register (28 items: 7 done, 21 open): `docs/implementation/ACTION-ITEMS.md`.
Items needing a decision now:

| ID | Description | Priority | Dependency | Responsible role | Target phase | Status |
|---|---|---|---|---|---|---|
| A-10 | Accept/reject RUSTSEC-2026-0459 exception; upgrade sqlx when it permits `libsqlite3-sys >= 0.39` | High | sqlx release | Product owner / Engineering lead | 1 (decision) / 2 | Open |
| A-27 | Commit Phase 1 work to version control (nothing committed by the implementer) | High | exit decision | Product owner | 1 | Open |
| A-11 | Select first production model provider | High | provider decision | Architect / Security | 2 | Open |
| A-26 | Run isolation suites on Linux and with Docker | Medium | A-08 | Platform engineer | 2 | Open |

## 14. Phase exit decision and rationale

**Proposed decision: READY WITH ACCEPTED EXCEPTIONS.**

Rationale: all 22 scope items are delivered; all 29 acceptance criteria are implemented and
verified with recorded evidence; the complete quality gate passes; no unresolved critical
defects remain; documentation, ADRs, threat model, test strategy, roadmap and action
register are current.

Exceptions requiring your acceptance:

| ID | Exception | Mitigation | Removal condition |
|---|---|---|---|
| E-1 | RUSTSEC-2026-0459 / CVE-2026-119375: bundled SQLite < 3.54 (via sqlx-sqlite 0.9.0, which caps `libsqlite3-sys < 0.38`); ignored in `deny.toml` | Local attack vector only (crafted DB + `-wal` pair); EAEH data dir `0700`, DB `0600`; documented in ADR-0003, THREAT-MODEL T24 | sqlx release allowing `libsqlite3-sys >= 0.39` (A-10) |
| E-2 | Container isolation verified only with Podman 5.5.1 on macOS; Docker and Linux hosts not exercised | Fail-closed tests runnable on any host; provider refuses when the runtime is unavailable | A-26 |

If E-1 is not accepted, the decision becomes **NOT READY – ACTION REQUIRED**. That would
block on an upstream sqlx release, or on an approved alternative such as a patched sqlx-sqlite
fork or a different SQLite driver. Either alternative would be an architectural change and needs an ADR.

Implementation completion, phase acceptance and authorization to proceed are separate
events. **Implementation has stopped. Phase 2 will not begin without your explicit
authorization.**
