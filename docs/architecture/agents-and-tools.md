# Agents, Delegation and the Tool Gateway

## Agent definitions (`eaeh-agents::definition`)

Versioned JSON documents (`config/agents/*.json`): id, version, purpose, responsibilities,
capabilities, skill refs (opaque until Phase 2), model preferences (capabilities, min
quality, preferred models, max output tokens), allowed tools, required context,
instructions prompt id, input/output JSON Schemas, autonomy level, delegation policy
(allowed targets, max children), per-instance budget limits, evaluation criteria.

Registration validates schemas, uniqueness, delegation sanity and that **every allowed tool
exists and its risk is within the agent's autonomy**. Versions are immutable (identical
re-registration is idempotent; different content ⇒ conflict). Status: active / deprecated
/ retired.

Shipped agents: `analyst` (L0, fs.read/list), `writer` (L2, + fs.write), `builder` (L3,
+ exec.run), `planner` (L1, no tools; output schema = plan proposal).

## Autonomy levels (deterministic)

| Level | Max tool risk |
|---|---|
| L0 read_only, L1 recommend | read_only |
| L2 workspace_modify | workspace_write |
| L3 execute_authorized | build_test |
| L4 external_side_effects | deployment (still subject to policy + approvals) |

## Lifecycle (`AgentRuntime`)

`instantiate` (hooks before/after; concurrency slot from workflow budget; instance budget
initialised; principal `agent:<uuid>` with **no roles**, bound to one project; audit +
`agent.created`) → `execute` → `terminate` (hooks; slot released; `agent.terminated`).
`run` = all three; termination always happens, also on failure.

### Execution loop

1. Validate task input against the definition's input schema.
2. Per step (budget `agent_steps`): assemble context — `system.agent-core` (System trust),
   the agent's prompt (AgentInstruction trust), the task input (TaskInput), and the
   transcript (model and tool outputs as **Untrusted** data).
3. Call `ModelClient` with routing requirements from the definition and the allowed tools'
   specs.
4. Proposed tool calls → loop detector → `ToolGateway::invoke` with the agent principal,
   the definition's allow-list and both budget owners. Denials and tool errors are returned
   to the model as data; `budget_exceeded`, `approval_required`, `supervision_halt` are fatal.
5. Final answer must parse as JSON and match the output schema; otherwise platform feedback
   is added and a bounded correction attempt is consumed.
6. Audit `agent.execute` with steps, tool invocations, model versions, prompts, corrections.

## Delegation (communication foundation)

`AgentRuntime::delegate(parent, target, task)` checks, in order: authorization
(`agent.delegate`), same scope, target in `delegation.may_delegate_to`, **cycle** (target
in lineage), **depth** (`models.max_delegation_depth`), **fan-out** (`max_children`),
**duplicate** task fingerprint within the same execution tree. Denials are audited.
Delegate/result messages are persisted (`agent_messages`, payload hashes only) and an
`agent.delegated` event is emitted. Model-initiated delegation (as a tool) is not exposed in
Phase 1.

## Tool registry

`ToolDescriptor`: id, semver version, description, risk class, input/output JSON Schemas,
isolation requirements, `needs_execution`, `needs_workspace`, timeout, rate limit,
idempotency, max retries (only idempotent tools may retry). Versions are immutable; callers
may request a version requirement. Tools contributed by plugins carry their plugin id as
source and are removed when the plugin is disabled/failed.

Built-in tools:

| Tool | Risk | Isolation | Notes |
|---|---|---|---|
| `fs.read` | read_only | in-process, workspace-confined | ≤ 1 MiB UTF-8 |
| `fs.list` | read_only | in-process, workspace-confined | hides symlinks and internal files |
| `fs.write` | workspace_write | in-process, workspace-confined | acquires a path lock for the caller, atomic write |
| `exec.run` | build_test | **all eight** capabilities ⇒ container provider | program allow-list (`isolation.exec_allowed_programs`) |

## Gateway pipeline (`ToolGateway::invoke`)

| # | Step | Failure |
|---|---|---|
| 0 | Resolve tool version | `not_found` (audited) |
| 1 | Agent allow-list (agents must present it) | `forbidden` |
| 2 | Authorization `tool.invoke` in scope | `forbidden` |
| 3 | Autonomy ceiling | `forbidden` |
| 4 | Input schema | `invalid_input` |
| 5 | Policy (`tool.invoke`, risk) | `policy_denied` |
| 6 | Require-approval → approval bound to tool+version+input fingerprint; otherwise request one | `approval_required` |
| 7 | Workspace scope match; isolation provider selection (refuse, never downgrade) | `forbidden` / `isolation` |
| 8 | Rate limit | `unavailable` |
| 9 | Budget `tool_calls` (agents must name an owner) | `budget_exceeded` |
| 10 | `before_tool_invocation` hooks (veto) | `hook_blocked` |
| 11 | Middleware pipeline → tool (timeout, idempotent retries) | tool error |
| 12 | Output schema, interceptors (redaction, 2 MiB limit) | `invalid_input` |
| 13 | `after_tool_invocation` / `on_tool_failure` hooks | — |
| 14 | Audit (`input_sha256`, policy rules, hooks, middleware, provider, duration) + `tool.invoked` event in one transaction | — |

Steps 0–9 are denial-audited and never depend on hooks (verified by
`mandatory_controls_hold_when_hooks_missing_disabled_or_failing`).

MCP: not integrated in Phase 1; an MCP client adapter would register tools in this registry
and remain subject to the gateway (master §13).
