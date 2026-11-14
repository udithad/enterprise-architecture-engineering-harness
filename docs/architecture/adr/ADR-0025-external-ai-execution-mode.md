# ADR-0025: External AI execution mode and governed software delivery

| Field | Value |
|---|---|
| Status | Accepted (amends ADR-0010, ADR-0023 and ADR-0024; plan approved 2026-11-13) |
| Date | 2026-11-13 |
| Phase | External AI execution and developer-experience enhancement |
| Deciders | Product owner (enhancement request), Principal architect |

## Context
Until now, every agent task ran a governed model loop inside EAEH (ADR-0010). Real AI work therefore needed an EAEH-managed provider with separate API credentials and billing. Developers who already pay for Claude Code or GitHub Copilot had to pay twice or use recorded fixtures (ADR-0023).

The enhancement request makes the developer's own AI client the default place where reasoning happens. EAEH remains the governance, context, workflow and orchestration layer. EAEH cannot use the client's model session or subscription credentials; only supported, explicit hand-offs over MCP are possible.

## Decision

### 1. Execution modes
`[execution] mode` is an installation setting.

| Mode | Model providers registered | Who reasons | Billing |
|---|---|---|---|
| `external_ai` | **none**, even if enabled in `[providers.*]` | the connected AI client | the user's own subscription, under its own terms and limits |
| `direct_provider` | the enabled providers (unchanged behaviour) | EAEH, via `ModelClient` | separate API credentials, billed by the provider |
| `deterministic` | replay only (ADR-0023) | nobody: recorded fixtures | none |

- The shipped `config/eaeh.toml` sets `external_ai`.
- An absent section means `direct_provider`, for backward compatibility.
- `deterministic` without a replay cassette is a configuration error.

### 2. Hand-off at agent-task level
In `external_ai` mode, `AgentRuntime` performs every governed step of an agent task except the model call.

1. **Prepare.** It resolves the approved skills, mandatory standards and knowledge excerpts, and assembles the context through the same `ContextAssembler` (trust ordering, redaction, data fencing, classification). It applies the `model.invoke` egress policy for the provider id `external-ai-client`, where restricted content is denied. The result is stored as an **external task package** in `external_tasks` (migration 0004). The package contains instructions, data, input, output schema, skill contracts, allowed tools, citable sources and rules.
2. **Suspend.** The runtime returns `ErrorKind::AwaitingExternal`. The engine treats this as a durable suspension, not a failure:
   - instance status `waiting_external`, task status `awaiting_external`;
   - `WorkflowData.awaiting_external` makes re-entry resume the same visit and task.

   The suspended agent instance keeps its identity and budgets.
3. **Work.** The client reads the package and may call the agent's **allowed tools through the tool gateway as the agent principal** (`external_tool`). These calls are authorized, policy-checked, budgeted, workspace-confined and audited. Citations they return become citable. Identical calls are limited.
4. **Submit.** `external_submit` records the result as untrusted output. `Harness::submit_external` then drives the workflow, and the task re-enters validation:
   - output schema, skill output contracts, citations and domain validators (exactly as in direct mode);
   - for implementation records, the listed files must exist in the workspace;
   - then the workflow's quality gate, artifacts, provenance and trace links.

   An invalid result reopens the task with feedback and consumes one correction attempt. When the budget is exhausted, the run fails.
5. **Provenance and separation of duties.**
   - Provenance records `models: ["external:<client>"]`, the package hash and the submitter.
   - The submitter is a **co-author**. Gate authors are now the union of all producers in the run, so a person who submitted work cannot approve it.
6. **Permission.** The new action `task.execute_external` is granted to `operator` only. Reading the package, calling task tools and submitting all require it. Listing hand-offs needs `workflow.read`.
7. **No fallback.** A runtime in another mode refuses to execute a task that is `awaiting` or `submitted` externally, and the run fails loudly instead of calling a provider. Planner-driven states and delegation need a model, so `external_ai` mode reports them as not supported.

### 3. Client integration
- One provider-neutral MCP server (`eaeh mcp`) adds `eaeh_task_list`, `eaeh_task_get`, `eaeh_task_tool` and `eaeh_task_submit`.
- The client label comes from MCP `clientInfo` and is used for provenance only.
- There is still no approval tool.
- `eaeh project init --client claude-code|github-copilot` writes these files:

  | Client | Files |
  |---|---|
  | Claude Code | `CLAUDE.md` section, `.mcp.json` |
  | GitHub Copilot (VS Code agent mode) | `.github/copilot-instructions.md` section, `.vscode/mcp.json` (`servers.eaeh`, `type: stdio`) |

  The instruction text is identical for both clients, and none of these files contains credentials.

### 4. Governed software delivery
- **Workflow `engineering.delivery`.** Approved design (requirements, HLD and LLD from the latest succeeded `engineering.design` run) → `plan` (agent `delivery-planner`, gate `delivery-plan`) → `approve_plan` → `implement` (agent `software-engineer`, autonomy `workspace_modify`, tools `fs.*`, gate `delivery`) → `snapshot` → `build_test` → `approve_delivery`.
  - `snapshot` uses `delivery.snapshot`, which produces a source bundle with `implemented_by` / `verified_by` trace links.
  - `build_test` runs `exec.run` in container isolation, with gate `build`.
  - The approved bundle reaches the repository through `eaeh artifact export`. The export reports whether exactly that version was approved.
- **Variant `engineering.delivery-unverified`.** Selected by the manifest setting `[delivery] build_verification = "deferred"`. It has no build step, and the bundle is marked "NOT EXECUTED BY EAEH". Readiness warns.
- **Activity.** `deliver` (aliases `plan`, `implement`, `test`) selects the variant. Readiness blocks it until a design has succeeded, and, in required mode, until a build command is allow-listed and container isolation is available.
- **Skills.** `implementation-planning` and `code-implementation` follow the normal skills lifecycle.

## Consequences
- **Cost.** In the default mode, no EAEH-managed model inference takes place and no EAEH model API key is needed. AI usage counts against the user's client subscription. EAEH's own infrastructure, storage and hosting costs are separate.
- **Execution model.** The client executes one hand-off at a time, as one conversational agent. EAEH does not claim parallel multi-agent execution in this mode.
- **Data egress.** Data handed to the client leaves EAEH, as it would for a direct provider. The same egress policy applies, and readiness warns for confidential BRDs.
- **Governance limits.** EAEH governs only what passes through it. A client can still edit repository files directly if a user asks it to. The generated instructions forbid this, but cannot enforce it.
- **Client labels.** The `clientInfo` label is self-declared.
- **Copilot.** GitHub Copilot's cloud coding agent cannot reach a local EAEH. Only VS Code agent mode, with a local stdio MCP server, is configured.

## Alternatives considered
- **Hand-off per model step.** The client would answer each turn of EAEH's agent loop. Rejected: many round trips and brittle request matching. The client already has its own agent loop.
- **Calling the client's model with its credentials.** Rejected: not supported, and a security violation.
- **Per-project execution mode.** Rejected for now: one database can be driven by several processes, and a mode stored per process could silently fall back. The project manifest records the *expected* mode, and readiness warns on a mismatch.
