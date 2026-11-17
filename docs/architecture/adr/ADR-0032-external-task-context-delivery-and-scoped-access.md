# ADR-0032: External AI task context delivery, task-scoped access and delivery-aware failure handling

Status: Accepted (amends ADR-0025 §hand-off and ADR-0026; Phase 1–3 scope)
Date: 2026-11-17

## Context

Workflow `01a1e921-de41-7505-ab94-2dd8e6b6981d` stopped twice. (1) `eaeh_task_get` returned the whole task package (102 KB analyse, 235 KB refine) in one MCP response; Claude Code diverted it to a file that the worker session cannot read (no file or shell tool, by design), so three attempts ended `no_submission`. (2) After sections were introduced, a running worker older than its MCP server launched Claude Code with a hand-kept `--allowedTools` list of the old tools; with `--permission-prompts none` every new tool was denied "for lack of approval". All attempts were charged as ordinary failures although the AI never had its task.

## Decision

1. **Delivery protocol, not a payload.** The package (immutable per `package_hash`) is delivered as a manifest plus deterministic sections read in bounded chunks (`offset`/`next_offset`/`done`, per-section sha256), repeated content sent once, large tool results paged through stored blobs. Nothing is truncated; above 16 MiB a hand-off is refused with the largest parts named.
2. **Acknowledgement.** Tools, validation and submission of the MCP server require an acknowledgement recorded from EAEH's own delivery record (every mandatory section completely served), optionally verified against the client's checksums.
3. **Task-scoped capability.** Each attempt receives a capability (`external_capabilities`, id = execution id) bound to tenant, project, workflow, task, stage and package version, expiring no later than the attempt's timeout (no margin), valid only while the attempt is running, and revoked when the attempt ends or the task is retried. All delivery operations require it and are audited, denials included. It authorizes **reading the task's own context only**; tools, validation and submission keep their existing authorization (agent allowed tools, operator role, budgets, deterministic validation) and approvals remain human decisions.
4. **No tool-list drift.** The worker allows the task-scoped MCP server as a whole (`mcp__eaeh`; the server exposes only the task tools and refuses other tasks) and verifies, before every attempt, that the server's tool list equals its own protocol (`eaeh mcp --task-tools`); a mismatch is `context_access_denied` with a restart remedy. Approval controls of every other server, tool and session are untouched.
5. **Failure model.** `context_delivery_failed`, `context_access_denied`, `context_integrity_failed`, `ai_execution_failed`, `no_submission`, `result_validation_failed`, `submission_failed`, `task_cancelled`. Delivery, integrity and access failures use neither an execution attempt nor a correction attempt; delivery/integrity are retried a bounded number of times (`max_context_attempts`), access denials are not retried. Client-side denials are read from the client's `permission_denials`.
6. **Recovery.** `eaeh worker retry <task> [--workflow <id>]` verifies the task↔workflow↔stage chain, keeps package version, submissions and accepted results, resets only dispatch/delivery counters and revokes stale capabilities.

## Alternatives rejected

* Raising the client output limit (`MAX_MCP_OUTPUT_TOKENS`): already set; the client still diverted the result.
* Writing the package to a file: assumes file access the session deliberately lacks.
* Disabling permission prompts globally or allowing all tools: violates least privilege; replaced by a server-scoped rule plus a build check.
* Truncating or summarizing the package: silently changes the task.

## Consequences

Large tasks need several MCP calls (about 8 for 100 KB). Capabilities attribute and bound task-context access but do not replace tool authorization. Real-client behaviour other than the verified Claude Code version is not guaranteed. Delivery traces add rows per attempt (`external_deliveries`, audit records per read).
