# External AI task context delivery, access and recovery

Scope: Phases 1–3 (external-AI hand-off, MCP, coordinator). Decision record: [ADR-0032](../architecture/adr/ADR-0032-external-task-context-delivery-and-scoped-access.md). Plans: [TASK-CONTEXT-DELIVERY-PLAN](stages/TASK-CONTEXT-DELIVERY-PLAN.md), [TASK-ACCESS-AND-RECOVERY-PLAN](stages/TASK-ACCESS-AND-RECOVERY-PLAN.md). User-facing protocol, failure names and recovery: [User Guide §8.1](../user-guides/EAEH-USER-GUIDE.md).

## Root causes (workflow `01a1e921-de41-7505-ab94-2dd8e6b6981d`, read from the database)

1. **Size.** `eaeh_task_get` returned the whole package in one MCP response (analyse 102 KB, refine 235 KB serialized). Claude Code diverted it to a file; the worker session has no file tool by design. Three analyse attempts and three refine attempts ended `no_submission`. `MAX_MCP_OUTPUT_TOKENS` was already raised and did not help.
2. **Duplication.** In refine, `input.inputs` and `input.outputs` are identical (43 KB each: the draft and the BRD) and `data` embeds the entire input and the output schema again: three copies.
3. **Version skew.** A worker started before the delivery change launched Claude Code with `--allowedTools` for the four old tools, while the MCP server it spawned (the rebuilt binary) exposed three new ones. With `--permission-prompts none` the new tools were denied "for lack of approval" (ten denials in the 11:44–11:47 attempts; the manifest call, an old tool, worked).
4. **Misattribution.** EAEH could not tell "the AI never had its task" from "the AI had it"; every failure consumed an execution attempt.
5. **Retry task.** `01a1e941-c58a-7333-9e60-3a45b97ad1c9` was verified to be the `refine` task of the same workflow (status `awaiting`, `dispatch_state=exhausted`, 0 submissions); the `analyse` task `01a1e921-de7e-…` is `accepted` with 1 submission and is never touched.

## Components

| Area | Change |
|---|---|
| `eaeh-agents/src/external/delivery.rs` (new) | Sections, chunking, de-duplication, delivery record, acknowledgement (with checksum and version verification), task capabilities, access gate and audit, paged tool results |
| `eaeh-store/migrations/0008…, 0009…` | `external_deliveries`, `external_blobs`, `external_capabilities`; trace columns on `external_executions`; `external_tasks.context_failures` |
| `eaeh-runtime/src/coordinator` | Failure classes (8), attribution from delivery/denial evidence, bounded delivery retries, capability issue/revoke per attempt, pre-attempt worker↔MCP build check, `retry_task_checked`, shared `TASK_MCP_TOOLS` |
| `…/coordinator/claude.rs` | `--allowedTools mcp__eaeh` (task-scoped server only), `permission_denials` captured |
| `eaeh-cli` | MCP tools `eaeh_task_get` (manifest), `eaeh_task_section`, `eaeh_task_ack`, `eaeh_task_blob`; ack gate on tool/validate/submit; `--execution`, `--task-tools`; `eaeh worker retry --workflow`; richer `worker history` |
| Portal | Execution and Task pages: delivery state, package size, sections retrieved, acknowledgement, denials, failure class, remedy |
| Tests | `tests/tests/context_delivery.rs` (15), adapter test, CLI/MCP test over the real binary, delivery unit tests |

Authorization model (unchanged elsewhere): an attempt's capability reads **only** its own task's context (tenant, project, workflow, task, package version), lasts at most the task timeout (no margin), works only while the attempt is `running`, and is revoked when the attempt ends or the task is retried. Tools, validation and submission keep their normal authorization; the worker's session allows only the task-scoped `eaeh` MCP server; approval gates are untouched.

## Verification (actual runs)

* `tests/tests/context_delivery.rs`, real databases and workflows, real coordinator and delivery code, client double with a 25 KB display limit and no file access: **15 passed**. Measured on isolated workflows (`EVIDENCE` lines): legacy single response / delivered mandatory bytes: 23.5 KB / 14.4 KB (small), 33 KB / 19 KB, **103 KB / 54 KB** (analyse-sized), **223 KB / ≈ 115 KB** (refine-sized), 620 KB class; no response above 25 KB.
* Covered: small inline, 102 KB, 235 KB-class, multi-document packages, oversized refusal, section and paged retrieval, missing mandatory sections, partial and interrupted retrieval, version/checksum integrity, capability scope (other task, other project, no capability), expiry, revocation, attempt end, audit of reads and denials, denial classification without retry, wrong version, submission failure class, delivery failures not consuming attempts, bounded retries then block, checked retry, worker-restart recovery, duplicate submission refused, downstream stage only after validation, worker↔MCP build mismatch.
* Real binary over MCP stdio (`eaeh-cli/tests/dx.rs`): manifest, every mandatory section read in ≤ 16 KB chunks, acknowledgement, tool, submission accepted.
* Workspace: see the final report for the last full run.

## End-to-end with the real Claude Code

An isolated installation and a copy of the Spring Boot project (BRD 67 KB, 81 sections) are prepared under the session scratch directory (own `EAEH_HOME`, data directory, project folder; your installation is untouched). The real run (analyse, then refine through the real `claude` CLI with the subscription login) needs the skills of that installation approved by a human approver; this is not done by the assistant. Status is stated in the final report.

## Known limitations

* The capability protects task-context reading and attributes it; it does not authorize tools (agent allowed tools and the gateway do).
* Only Claude Code (the installed version) is exercised against the real client; GitHub Copilot interactive sessions use the operator authorization, not capabilities.
* Section delivery stores one record per attempt and writes an audit record per read (a 235 KB task ≈ 20 records).
* `submission_failed` is recorded when the MCP server sees a non-conflict error while submitting; other infrastructure failures surface as `ai_execution_failed`.
* Package size cap (16 MiB) is a constant, not yet a setting.
* Intermittent in the workspace run, outside this change: `portal_operations::operations_need_the_operator_identity_and_backend_authorization` (resume of a cancelled run) passed in 2 of 3 repeated runs; it was not compared with a baseline.
