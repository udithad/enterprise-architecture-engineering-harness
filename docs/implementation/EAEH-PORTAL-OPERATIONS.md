# EAEH portal operations: project onboarding, workflow operations, recovery and restart

| Field | Value |
|---|---|
| Status | Implemented 2026-11-17 (supplementary enhancement within Phases 1–3; roadmap unchanged) |
| Source | `dev-instructions/workflow-improvement-stp1-3.md` |
| Plan | `docs/implementation/stages/PORTAL-OPERATIONS-PLAN.md` (W1–W4 approved) |
| Decision record | ADR-0031 (amends ADR-0026 §4) |
| Inventory | `docs/implementation/EAEH-PORTAL-CLI-PARITY.md` |

## 1. What was built

| Area | Backend (reused / added) | Portal |
|---|---|---|
| Register existing project (§1) | `runtime::onboarding`: `confine`, `list_dir`, `detect` (read-only). `cli::dx::register_project` is the former body of `project init`, now a service with no printing, called by `eaeh project init` and by the portal through `PortalHost`. | **Projects → Register existing project**: browse the workspace roots, detection, review and edit, register, result with readiness and **Open project** |
| Start workflows (§2) | `GET /v1/projects/{id}/plan` (= `project::plan_activity`, the `workflow run` planner); `POST /v1/workflows` (existing); `POST /portal/projects/{id}/worker` (= `worker::ensure_worker`) | **Project → Start workflow**: definitions from the backend, execution plan (stages, agents, skills, approvals), input, readiness, start |
| Resume, restart, retry (§3) | `WorkflowEngine::restart_plan`, `restart`, `lineage`, `ancestors`; `check_gate` with carried approvals; `Harness::authorize_resume`, `resume_workflow`, `restart_workflow`; `coordinator::retry_task` (existing). `POST /v1/workflows/{id}/resume`, `…/restart/preview`, `…/restart`, `GET …/lineage`, `POST /v1/tasks/{id}/retry`. CLI: `eaeh workflow restart`. | Workflow page: operations bar (**Resume**, **Restart from stage…** / **Retry failed stage…**, **Cancel**), restart preview dialog, **Lineage** card. Task page: **Retry AI hand-off**. |
| Parity (§4) | `tests/tests/parity.rs` parses the CLI tree | Inventory document (77 operations) |
| Dashboard (§5) | `/v1/dashboard` with `pending_action`; `/v1/workflow-queue` (filters: definition, stage, run status, since; `pending_action`, `current_task`, `operations`); `/v1/task-queue` (filters and keyset cursor) | Dashboard: pending-action column, links to project and task, **Recently executed tasks**. Workflows: filters. **Tasks** page with filters and paging. |
| Detail views (§6) | `/v1/workflows/{id}/detail` (+ `lineage`, `operations`); `GET /v1/tasks/{id}` | Workflow page (lifecycle, tasks linked, approvals, documents, governance, history, failure and recovery, lineage); **Task** page (input, agent and skill versions, dependencies, attempts, hand-off, output, artifact, validation, errors, retry and re-execute) |
| Execution and integrations (§4, §8) | `GET /v1/executions` (attempts, workers, hand-offs), `GET /v1/integrations` (mode, billing, clients, client files, workers, `direct_model_calls`) | **Execution** page |
| Knowledge, agents, tools | Existing `/v1/knowledge/*`, `/v1/agents`, `/v1/tools` (now on the portal read allowlist) | **Knowledge & agents** page (read-only) |
| Governance | Existing `/v1/audit/verify`, `/v1/health/details` | **Settings**: identities, workspace roots, health, audit-chain check |

## 2. Restart semantics (W1, ADR-0031)

```
source run R1  brd → analyse → refine → render_req → approve_req ✔ → hld → render_hld → approve_hld ✔ → lld → approve_lld (waiting)
restart R1 at "hld"  (preview: fingerprint F)
new run   R2  [brd … approve_req carried, same versions, approval verified via lineage] → hld → render_hld (v2) → approve_hld (new decision) → lld → approve_lld
R1: cancelled, "superseded by R2" – data, artifacts, approvals unchanged
```

* **Allowed restart points:** stages on the definition's main path up to the first gate that is not validly approved. An unknown stage is `invalid_input`; a later stage is `conflict` and names the gate that would be skipped.
* **Changed input:** only from the first stage; nothing is carried.
* **Idempotency:** one derived run per (source, stage, fingerprint). A stale fingerprint is `conflict`.
* **Recovery:**
  * resume re-drives a non-terminal run from its persisted state: an approval wait, an AI hand-off, or a blocked document write;
  * start-up recovery (`Harness::start_background`) is unchanged;
  * a hand-off a worker gave up on (`blocked`, `exhausted`) is made eligible again with retry.
* **External AI:** unchanged. A restart creates new hand-offs only for re-executed stages. EAEH makes no model call in `external_ai` mode; `GET /v1/integrations` reports `direct_model_calls` from the configured mode.

## 3. Security

* **Identities:** operations need `--operator` (and `workflow.start` in the backend); registration needs `--admin` (`principal.manage`). The viewer never changes state.
* **Portal allowlist:** classified per request (`Read`, `OperatorRead`, `Decision`, `SkillRegister`, `SkillScan`, `Operate`). Tokens, principals, plugins, knowledge ingestion and deletion, and standards decisions stay refused.
* **Portal-process endpoints:**
  * they need the launch-secret session, the loopback `Host`, and the same `Origin` on writes;
  * paths are canonicalized and confined to the workspace roots; symlinks out of a root and hidden folders are not listed;
  * document paths must stay inside the project folder.
* **Isolation:** another project's runs, tasks, lineage and plans answer `not_found`; task-queue lists only the projects the caller can access.
* **Secrets:** new people's tokens go to the local credential store and never reach the browser; tests assert that no stored token appears in any response. Hand-off packages are not part of the read models; execution details are the recorded, redacted excerpts.
* **Audit:**
  * `workflow.resume`, `workflow.restart`, `workflow.superseded`;
  * `external_task.retry`;
  * project registration's existing audit records (project, principals, knowledge, skills, workspace);
  * cancel's existing `workflow.cancel`.
* **The portal stays local-development only.** There is no login; it is not production-ready (ADR-0026).

## 4. Schema
Migration `0007_workflow_lineage.sql`: a new table `workflow_lineage` with the `UNIQUE (source_instance_id, restart_stage, plan_fingerprint)` constraint and an index on the source. `WorkflowData` gains the optional fields `carried_approvals` and `restarted_from`, which existing rows deserialize unchanged. No other schema change, no new database.

## 5. Test evidence

See §13 of the delivery report in the conversation, and the commands below:

| Scenario (§10) | Test |
|---|---|
| 1, 2 register a Spring Boot folder, detect structure and documents | `onboarding::tests::detects_a_spring_boot_project_and_its_documents`; `portal_operations::register_existing_project_is_confined_validated_and_needs_the_admin`; real-binary E2E (`e2e-onboarding.sh`) |
| 3 start the design workflow through the portal | `portal_operations::portal_restarts_from_hld_…` (definitions from the backend, `POST /api/v1/workflows` as the operator, run reaches the Requirements gate, `started_by` = operator); refused without `--operator`; vitest `Start workflow` (plan, readiness gating, payload) |
| 4 view every stage and task | `portal_operations::portal_restarts_from_hld_…` (detail, task page, task queue) |
| 5, 6 approve Requirements and HLD, LLD generated | `portal_operations::portal_restarts_from_hld_…`, `restart.rs`, existing `portal.rs` |
| 7 resume a run waiting for external AI | `portal_operations::external_ai_wait_is_resumed_and_a_stopped_handoff_is_retried` |
| 8 recover after a worker interruption | the same test (blocked hand-off → retry); existing `automated_ai.rs` (lease expiry, retry, backoff) |
| 9 retry a failed task | `restart::a_failed_stage_is_retried_in_a_derived_run` |
| 10 restart from an earlier stage | `restart::restart_from_hld_…`; `portal_operations::portal_restarts_from_hld_…` |
| 11 upstream change → downstream invalidation | `restart::restart_points_never_skip_approvals_or_leave_the_definition` (new input from the first stage, all approvals repeated); restart from HLD regenerates HLD and LLD and requires new approvals |
| 12 approved artifacts not silently overwritten | `restart::restart_from_hld_…` (old HLD readable, same hash, new version id) |
| 13 CLI and portal consistent | real-binary E2E (same installation: `project list` and `project readiness` against the portal overview and plan) |
| 14 parity inventory complete | `parity::every_cli_operation_is_classified_once_with_reasons_for_exclusions` |
| 15 authorization, audit, isolation | `portal_operations::operations_need_the_operator_identity_and_backend_authorization`, `project_isolation_hides_runs_tasks_and_lineage`, `restart::…_unrelated_run_is_rejected` |

Commands: `scripts/ci.sh`; `cargo test -p eaeh-acceptance --test restart --test portal_operations --test parity`; `npm --prefix portal test`.

### Results (2026-11-17)

* **`scripts/ci.sh`:** **CI PASSED**. 327 Rust tests passed, 0 failed; fmt and clippy `-D warnings` are clean; cargo-deny reports advisories, bans, licenses and sources ok.
  * New tests: `restart` (4), `portal_operations` (5), `parity` (1), and the `onboarding` unit tests (2).
* **Portal:** `npm test` passed 49/49 (9 new in `operations.test.tsx`); `npm run build` succeeds.
* **Parity:** 77 CLI operations: Action 14, Read 24, CLI-only 31, N/A 8.
* **Real-binary E2E** (`target/debug/eaeh`) on an **isolated** installation: deterministic mode, its own data directory and credential store, port 8291.
  * **Test project:** a copy of `simple-springboot-api-b`'s BRD and bootstrap notes, plus a Spring Boot `pom.xml` and source skeleton labelled TEST DATA, with `git init`.
  * **Browse and detect** (portal started with `--admin admin`):
    * the folder is listed and marked as a project;
    * `/etc` → 403; no session cookie → 401;
    * detection: Java / Spring Boot / Maven, Git `refs/heads/main`, BRD `docs/BRD-Simple-Spring-Boot-API.md`, README, registration `new`.
  * **Register:**
    * a document path outside the folder → 400;
    * register → 201: project created, people `dev`, `steward` and `approver` created with profiles stored locally, BRD and knowledge ingested, skills registered (pending), manifest, workspace and client files written;
    * a duplicate → 409 (already registered).
  * **Workspace:**
    * `.eaeh/project.toml`, `CLAUDE.md` and `.mcp.json` are present;
    * the source-code hash is unchanged, and no token is in the workspace.
  * **CLI and portal on the same installation (scenario 13):**
    * `eaeh project list` shows the project;
    * `eaeh project readiness` is NOT READY (exit 7, skills await human approval). The portal (`--operator developer`) plan for `engineering.design` (11 stages, 3 approvals, BRD citation as input) reports the same blocking skills for that workflow. The CLI's total also includes the delivery workflow.
  * **Refusals:** a registration without `--admin` → 403; `POST /v1/tokens` → 403.
  * **`simple-springboot-api-b` untouched:** its tree hash is the same before and after.
* **Not run on the real binary:** the workflow phase (start, Requirements and HLD approvals, restart from HLD). It needs the human approver to approve the skills of the isolated installation first, and EAEH approvals are made by people. These scenarios are covered by the harness tests above, through the real portal application, engine and approval service.
