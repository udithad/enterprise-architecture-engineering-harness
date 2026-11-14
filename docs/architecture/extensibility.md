# Hooks, Middleware, Interceptors, Events and Plugins

Distinct responsibilities (ADR-0005):

| Mechanism | Interface | Purpose | Effect on outcome | Failure semantics |
|---|---|---|---|---|
| Hook | `Hook::on_event(&HookContext) -> HookOutcome` | React to lifecycle points | Veto at `Before*` points only (restrict) | Per registration: `block` aborts, `warn` records, `continue` ignores; timeouts are failures |
| Middleware | `Middleware<Req,Res>::handle(req, next)` | Cross-cutting around a pipeline | May short-circuit or transform | Errors propagate; pipeline timeout |
| Interceptor | `Interceptor<T>::intercept(value)` | Transform a value at a boundary | Data only | Error aborts the crossing |
| Event | `EventSubscriber::handle(&EventEnvelope)` | Decoupled notification after commit | None (after the fact) | Retry with backoff; dead-letter after N attempts |

## Hook points (42) and emitters in Phase 1

| Category | Points | Emitted by (Phase 1) |
|---|---|---|
| Agent | before/after instantiation, execution, termination | `AgentRuntime` ✔ |
| Task & workflow | before/after task execution, before/after workflow transition, before/after approval, on rejection/cancellation/timeout/failure/recovery | `WorkflowEngine` ✔ |
| LLM | before/after model request, before/after response processing | `ModelClient` ✔ |
| Tool | before/after invocation, on failure | `ToolGateway` ✔ |
| Context | before/after context assembly | `ContextAssembler` ✔ |
| Knowledge | before/after knowledge retrieval | **contract only** – no retrieval until Phase 2 |
| Engineering – artifacts/files | before/after artifact creation, before/after file modification | `ArtifactStore`, `Workspace` ✔ |
| Engineering – build/test/commit/PR/deployment | before/after each | **contract only** – emitters arrive with Phases 4–7 |

Registration: id, version, hook-API requirement (`^1`), points, priority (ascending, ties
by id), failure policy, timeout (≤ 300 s), enabled flag, filter (subject exact/prefix,
tenants), source (`core` or plugin id). Dispatch returns a report (executions, warnings,
block reason) that callers include in audit details (e.g. tool invocations list
`hook_id@version`).

## Middleware

`Pipeline<Req, Res>` (deterministic order by `order()` then name; duplicate names rejected).
The tool gateway owns a `tool-invocation` pipeline; plugins with `register_middleware` can be
granted access in a later phase (no plugin middleware registration API is exposed in Phase 1).

## Interceptors

`RedactionInterceptor` (strings & JSON; sensitive keys, registered secrets, token patterns)
and `SizeLimitInterceptor` are installed on the gateway's output chain by the runtime.

## Events

Envelope: `event_id`, `event_type`, `schema_version`, `occurred_at`, tenant, project,
`correlation`, `causation`, producer, payload. Catalogue (schema v1): `agent.created`,
`agent.execution_started`, `agent.execution_completed`, `agent.terminated`,
`agent.delegated`, `task.assigned`, `tool.invoked`, `policy.evaluated`,
`approval.requested/granted/rejected`, `artifact.generated`, `validation.failed`,
`workflow.started/transitioned/completed`, `budget.exhausted`, `supervision.escalated`,
`plugin.state_changed`, `model.invoked`. (`task.assigned`, `policy.evaluated` and
`budget.exhausted` are defined but not yet emitted — TD-04.)

Delivery: transactional outbox (`INSERT OR IGNORE` by event id ⇒ duplicate publication
ignored); `EventDispatcher` processes pending events in order; per (event, consumer)
delivery state `pending → done | dead`; exponential backoff with jitter; dead letters are
audited (`event.dead_lettered`); `ordered()` consumers never skip ahead. The runtime
dispatches every `supervision.dispatch_interval_ms`.

## Plugins

See ADR-0008. Lifecycle: `validated → enabled ⇄ disabled → retired`; `enabled → failed →
disabled|retired`. Manifest: id, version, kind (`in_process` | `wasm` | `external_process`),
description, `eaeh_api` requirement, dependencies, capabilities (`register_hooks`,
`register_tools`, `register_middleware`, `register_interceptors`,
`register_model_providers`, `register_validators`), config schema, resource limits,
integrity (digest, signature, key id).

Activation at startup: `register_compiled` (verifies, idempotent for an identical manifest,
rejects a changed manifest under the same version) → if `enabled` in the database, call
`register()` with an `ExtensionContext` exposing only granted registries → on error, roll
back contributions and mark `failed`. Enable/disable/retire via API or CLI by an Admin take
effect on the next start.
