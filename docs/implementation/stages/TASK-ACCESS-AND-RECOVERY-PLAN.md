# External AI Task Access, Authorization and Recovery – Plan (DRAFT, awaiting approval)

Scope: Phases 1–3. Extends [TASK-CONTEXT-DELIVERY-PLAN.md](TASK-CONTEXT-DELIVERY-PLAN.md) (manifest, sections, acknowledgement, blobs, delivery trace – implemented). No direct provider calls, no Agent SDK billing, no hidden fallback, no global permission change.

## 1. Root causes (read-only inspection of workflow `01a1e921-de41-7505-ab94-2dd8e6b6981d`)

| Failure | Evidence | Root cause |
|---|---|---|
| Analyse: three `no_submission` | executions 10:51–10:53, ~102 KB | Whole package in one MCP response; Claude Code diverted it to a file it cannot read (fixed by sections, earlier change) |
| Refine: 235 KB, then "denied, needs approval" ×10 | executions 11:26–11:28 (old protocol) and 11:44–11:47 (`eaeh_task_get` manifest worked, section reads denied) | **Two causes.** (a) Size: refine `input` carries the draft (27 KB) + BRD (16 KB) **twice** (`inputs` and `outputs` are identical) and `data` re-embeds the whole input a **third** time: 193 KB stored, 235 KB serialized. (b) **Version skew**: the running worker was started before the delivery change; it launched Claude Code with `--allowedTools` of the four old tools, while the MCP server it launches is the rebuilt binary on disk with three new tools. With `--permission-prompts none`, every unlisted tool is *denied as needing approval*. The skew is silent because the allow-list lives in the worker and the tool list in another binary |
| Retry task `01a1e941-c58a-7333-9e60-3a45b97ad1c9` | `external_tasks` | Verified: it is the `refine` task of **this** workflow, status `awaiting`, `dispatch_state=exhausted`, 0 submissions; the `analyse` task `01a1e921-de7e-…` is `accepted` (1 submission). The workflow waits at `refine`; nothing downstream started |
| Classification | all rows `no_submission` | EAEH could not tell "AI never had its task" from "AI had it": tool denials were invisible (the client's result event lists `permission_denials`, EAEH ignored it) |

## 2. Design

1. **No more skew (permissions).** The worker allows the **task-scoped server as a whole** (`mcp__eaeh`; it exposes only the task tools) instead of a hand-maintained list, and `eaeh mcp --task-tools` prints the server's tool names. Before every attempt the coordinator compares that list with the tools its own protocol needs; a mismatch stops the attempt as `context_access_denied` with the remedy "restart the worker (`eaeh worker stop && eaeh worker start`): worker and MCP server are different builds". MCP approval controls elsewhere (interactive sessions, other servers, other tools) are untouched; privileged operations are not in the task-scoped server at all.
2. **Task-scoped capability.** New table `external_capabilities` (one per attempt): `id=<execution id>`, tenant, project, workflow instance, task, stage, **package_hash**, `expires_at` (attempt timeout + margin), `revoked_at`, `revoked_reason`. Created when the attempt starts; revoked when it ends (success, failure, cancel) or when the task/workflow is cancelled or superseded. Every delivery operation (manifest, section, blob, ack, task tool/validate/submit through MCP) requires a live capability for exactly this `(task, execution, package_hash)` and the caller's tenant/project; otherwise `context_access_denied` (`expired`, `revoked`, `wrong_task`, `wrong_package`). Capabilities grant **read of the task's own context only**; tools/submit keep their existing authorization (agent allowed-tools, operator role, budgets, validation) and human gates are unchanged. Interactive (non-worker) clients keep the existing operator authorization and are not issued capabilities.
3. **Audit every access.** One audit record per delivery operation (`external_task.context_read` with section, offset, bytes, decision allow/deny and reason; `external_task.context_acknowledged`), including denials. Per-attempt counters in `external_deliveries`: requests, bytes, denied count.
4. **Package version everywhere.** `eaeh_task_section` / `eaeh_task_blob` accept `package_hash`; a mismatch is `context_integrity_failed`. `eaeh_task_ack` accepts the section checksums the client computed and rejects any difference (`context_integrity_failed`). Manifest already carries per-section sha256 and byte counts; section order is fixed.
5. **Failure classes** (all eight requested): `context_delivery_failed` (transport/partial/never requested), `context_access_denied` (capability or approval denial; **not retried**, blocks with remedy), `context_integrity_failed` (hash/version mismatch; one re-delivery try), `ai_execution_failed`, `no_submission`, `result_validation_failed`, `submission_failed` (infrastructure error while submitting: the AI did submit), `task_cancelled`. The Claude adapter reads `permission_denials` of the result event so denials are classified even though they never reach EAEH. Only `no_submission` / `ai_execution_failed` / `result_validation_failed` consume execution or correction attempts. Transient delivery failures: bounded retry with backoff (already); denials: no retry.
6. **Smaller packages.** Delivery encoding already removes the `inputs`/`outputs` duplicate and the copies of input/output schema inside `data`; add: `data` fenced items identical to another section are replaced by pointers, and `data` is a *reference* section. Expected refine size 235 KB → ≈ 50 KB mandatory. Warnings: manifest `warnings` when a mandatory section exceeds 128 KB or the package exceeds 512 KB; hard cap 16 MiB (already).
7. **Recovery.** `eaeh worker retry <task> [--workflow <id>]` verifies the task belongs to the stated workflow, prints task → stage → workflow → package hash → accepted predecessor, refuses when the task is accepted/cancelled or the workflow no longer waits at that stage, resets only the delivery/dispatch counters (never submissions or accepted results), keeps the same package (hash unchanged) and revokes stale capabilities. Duplicate submissions: a second submit for an accepted task is refused idempotently (`already accepted`, same hash returns the original result); downstream stages start only through the engine after validation (tested).
8. **Observability.** `external_executions` gains `capability`, `denied_calls`, `sections_total`, `sections_retrieved`, `acknowledged_at`; `eaeh worker history`, `eaeh task show`, Portal *Execution* and *Task* pages show package size, version, sections retrieved/total, acknowledgement, denials, failure class and remedy, so a failure is attributable to delivery, authorization, AI execution or submission without logs.
9. **MCP discoverability.** Tool descriptions state when/how to use each tool, arguments and response shape; errors name the missing permission/resource and the next tool to call; `eaeh_task_get` returns a `protocol` array (the exact step list).
10. **Docs.** Protocol, large packages, authorization model, acknowledgement, failure classes, retry/recovery, troubleshooting in `EAEH-USER-GUIDE.md` and a new `EAEH-TASK-CONTEXT-DELIVERY.md`.

Migration `0009_task_capabilities.sql` (additive).

## 3. Tests (isolated; the real workflow and its artifacts are never touched)

Unit and integration (acceptance crate, real DB, real coordinator): 0.1/50/102/235 KB and multi-document packages; section and paged retrieval; missing mandatory section; partial/interrupted retrieval and resume; capability scoping (other task, other project, wrong package hash), expiry, revocation, denial audit; no approval needed for authorised reads; denial classification without retry; acknowledgement; delivery failures not consuming attempts; successful submission; worker restart and recovery; duplicate submission; workflow progression only after validation; version-skew detection.

Adapter tests: fake `claude` programs (existing harness) that report `permission_denials` and speak MCP to the **real `eaeh mcp --task` binary** with a response display limit.

## 4. End-to-end acceptance

Isolated EAEH installation (own `EAEH_HOME`, data dir, port) and an **isolated copy of the simple Spring Boot project**: `eaeh project init`, start the requirements activity, background worker with the real Claude Code. Verify analyse and refine (235 KB-class package from the BRD + draft) each: every mandatory section retrieved, no truncation, no approval prompt, acknowledgement recorded, validation passes, workflow reaches the requirements approval gate (which I do **not** decide: human-only). Evidence from `eaeh worker history`, the delivery trace and the audit log.

## 5. Decisions needed

1. Run the acceptance with the **real Claude Code CLI** on your subscription (the CLI here reports a signed-in `pro` subscription login; two AI stages, typically several minutes, counted against your usage limits, no API key, no model API billing), or only with scripted fake `claude` programs?
2. Capability lifetime: attempt timeout (30 min) + 5 min margin, revoked when the attempt ends. OK?

## 6. Limits to document

* Real-client behaviour beyond the acceptance run (other Claude Code versions, GitHub Copilot interactive sessions) is not verified; Copilot keeps the interactive operator authorization.
* Capabilities protect task-context access and attribute it; they do not add authorization for tools, which stays with the agent's allowed tools and the gateway.
