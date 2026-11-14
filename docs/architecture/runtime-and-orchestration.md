# Runtime, Orchestration, Persistence and Reliability

## Hybrid orchestration

| Concern | Deterministic engine (`eaeh-workflow`) | Intelligent orchestration (`eaeh-runtime::executor`, `eaeh-agents`) |
|---|---|---|
| Lifecycle, transitions, execution order | ✔ authoritative | — |
| Approval gates, gate prerequisites | ✔ | — |
| Policy evaluation of plan steps | ✔ (`workflow.plan_step`) | — |
| Retry, timeout, cancellation, recovery | ✔ | — |
| Task decomposition | validates | `AgentPlanner` proposes (`Untrusted<PlanProposal>`) |
| Agent selection & instantiation | requests capabilities + autonomy ceiling | `AgentTaskExecutor` discovers & runs |
| Tool selection | — | model proposes; gateway decides |

The engine depends only on `eaeh-governance`, `eaeh-extensibility`, `eaeh-workspace`
(and below). It drives intelligent work through two traits:

```rust
trait TaskExecutor { async fn execute(&self, t: TaskExecution) -> Result<TaskOutput>; }
trait Planner      { async fn propose(&self, ctx: PlanningContext) -> Result<Untrusted<PlanProposal>>; }
```

## Workflow definitions

Declarative JSON (`workflows/*.json`), validated on registration and immutable per version.
State kinds: `agent_task`, `tool_step` (deterministic, executed by the engine's service
identity `system:workflow-engine`, Operator role confined to the project), `planned`,
`approval_gate`, `terminal`. Triggers: `succeeded`, `failed`, `timed_out`, `approved`,
`rejected`. Validation: unique names, valid targets, gates with approved+rejected
transitions reviewing an artifact-producing state, `requires_approval_of` referencing gates,
retry bounds, reachable terminal state.

Phase 3 additions (ADR-0020, ADR-0021):
- **Trigger and revisions:** new trigger `changes_requested`. If a gate has no such transition, it falls back to `rejected`. Gates have `max_revisions`, default 3.
- **Bindings:** agent-task `inputs`, tool-step `input` and gate `context` may contain `$output`, `$output` with `pointer`, `$input` or `$artifact`. Bindings may only reference work states.
- **Quality gates:** a work state can declare `quality_gate: {gate, context}`. The gate must be a platform-configured gate (`requirements`, `hld`, `lld`).
- **Bundles:** `bundle: true` turns `{files}` output into one artifact per file plus a manifest.

Shipped definitions:
- `demo.approval`: deterministic, no LLM (write → review → publish).
- `engineering.task`: plan → design → review → implement; requires a model provider.
- `knowledge.review` (Phase 2).
- `requirements.analysis` and `engineering.design` (Phase 3; see [engineering-design.md](engineering-design.md)).

## Execution semantics (`WorkflowEngine::drive`)

* A per-instance async lock serializes drives in-process; optimistic `version` checks
  protect across processes.
* **Work states**: gate prerequisites checked first (`requires_approval_of` → approval must
  be `approved`, for this instance and gate, in scope — else instance `failed`, audited
  `workflow.gate_check denied`). Visit counter incremented. Task row created with an
  idempotency key `instance:state:visit:attempt[:step]`; a `succeeded` task with the same
  key is replayed, never re-executed. Hooks `before/after_task_execution`. Execution under
  `tokio::select!` of state timeout and cancellation token. Retryable failures (including
  timeouts) retried up to `retry.max_attempts` with linear backoff. Budget exhaustion or
  supervision halts without a `failed` transition escalate the instance.
* **Outputs**: stored in workflow data; if the state names an artifact, stored as a new
  version in `ArtifactStore` with provenance (agent, model versions, prompts, tools,
  context snapshot, workflow, task).
* **Approval gates**: create one approval per visit (supersedes pending), bound to the
  artifact id + SHA-256; authors (agent principals) recorded for separation of duties.
  Instance status `waiting_approval`. `record_decision` → `ApprovalManager::decide` (all
  checks there) → hooks `before/after_approval`, `on_rejection` → transition. Rejection
  stores comments as `revision_feedback` and routes back for revision; history retained.
  Phase 3:
  - **`changes_requested`:** comments become `revision_feedback`. Counted against `max_revisions`; once the limit is exceeded, the gate follows `rejected`. Audited as `workflow.revision`.
  - **Quality gates:** run on a work state's output *before* it is stored. They fail closed, are audited (`workflow.quality_gate`), emit `validation.failed`, and record the result in provenance.
  - **Trace links:** declared by an approved artifact (`trace`). Recorded only when its gate is approved (`trace_links`).
* **Transitions**: one write transaction updates the instance (optimistic version), appends
  `workflow_transitions` (with a full data checkpoint), appends audit, enqueues
  `workflow.transitioned`. Hooks `before/after_workflow_transition` (a blocking veto fails
  the instance).

## Durable state and recovery

`WorkflowEngine::recover()` (called by `Harness::start_background`) marks `pending`/`running`
tasks `interrupted`, emits `on_recovery` hooks, audits `workflow.recover`, and the runtime
resumes each `running` instance. Completed steps are replayed from idempotency records and
checkpoints; only the interrupted step re-executes (verified by
`interrupted_workflow_recovers_without_duplicate_side_effects`). Waiting instances stay
waiting.

## Supervision and budgets

* `BudgetLedger`: per-owner durable limits (`tool_calls`, `model_calls`, `tokens`,
  `cost_micros`, `wall_clock_ms`*, `delegation_depth`*, `concurrent_agents`,
  `correction_attempts`, `agent_steps`). Atomic, all-or-nothing across owners (agent +
  workflow); unconfigured resource ⇒ fail closed. (*declared; wall-clock is enforced via
  state/tool timeouts and delegation depth via runtime configuration — see TD-03.)
* `Supervisor::scan_once` (background, `supervision.interval_secs`): running tasks without
  heartbeat for `stall_after_secs` ⇒ task `escalated`, instance `escalated`, audit +
  `supervision.escalated` event; instances with ≥ `max_failed_tasks_per_instance` failed
  tasks ⇒ escalated.
* Agent loop: identical tool call repeated > 3 times ⇒ `supervision_halt`; step budget;
  bounded correction attempts.

## Persistence

See [data.md](data.md) and ADR-0004: SQLite/WAL, one serialized `BEGIN IMMEDIATE` writer,
state + audit + outbox in one transaction.

## Observability

* Structured JSON logs (`tracing`) through `RedactingMakeWriter` (registered secrets and
  `eaeh_…`/`Bearer …` patterns removed). Each API request logs request id, method, path,
  status, latency; responses carry `x-request-id`.
* Correlation: every workflow has a correlation id carried into tasks, agents, model calls,
  tool invocations, artifacts, approvals (decisions are recorded under the workflow's
  correlation) and events; `Db::audit_by_correlation` reconstructs a run.
* Metrics: `MetricsSubscriber` counts events by type, tool successes/failures, model
  tokens, workflow completions; `GET /v1/metrics` (Prometheus text) + outbox backlog gauge.
* Health: `GET /v1/health` (public, minimal) and `/v1/health/details` (authorized):
  database, migrations, isolation providers, model providers, active plugins, pending
  events.
* OpenTelemetry export: not implemented (optional, D-05).
