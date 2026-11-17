# Portal operations: project onboarding, workflow operations, recovery and restart, CLI parity – plan

| Field | Value |
|---|---|
| Status | IMPLEMENTED 2026-11-17: W1–W4 as recommended (ADR-0031; evidence in `docs/implementation/EAEH-PORTAL-OPERATIONS.md`) |
| Date | 2026-11-16 |
| Source | `dev-instructions/workflow-improvement-stp1-3.md` |
| Scope | Supplementary enhancement within Phases 1–3. No roadmap changes. It changes ADR-0026 (the portal becomes able to operate, not only read and decide) and the rule "a failed run is final". Both changes need your approval: W1, W2. |

## 1. Inspection (from source)
* **CLI.** 24 top-level commands, 76 operations: `init`, `health`, `serve`, `portal`, `admin`, `audit`, `workflow` (start, run, watch, status, resume, show, list, cancel, definitions), `approval`, `token`, `principal`, `project` (init, readiness, create, list, set-domain), `plugin`, `knowledge` (9), `skill` (5), `brd`, `trace`, `artifact` (status, sync, export), `auth` (5), `task` (5), `mcp`, `worker` (7), `review`, `agents`, `tools`.
* **API.** `/v1` already has: start and cancel workflows, definitions, approvals and decisions, artifacts, agents, tools, audit, tokens, principals, projects, plugins, knowledge, skills, and traceability. Portal read models were added in ADR-0026 to ADR-0029.
* **Portal (ADR-0026 allowlist).** Reads; decisions (approver); skill registration (steward); skill scan. Everything else is refused.
* **Engine.**
  * `start` (with input);
  * `drive` (= resume a non-terminal run);
  * `cancel`;
  * `recover` (re-drives `running` runs after a restart);
  * an approval gate with a revision loop (`changes_requested` reruns the producing stage).

  **Missing:**
  * retry of a failed run (failed is terminal: "start a new run");
  * restart from a chosen stage;
  * lineage between runs.

  Approvals bind to `"{run}:{gate}"`, and the completion guard verifies that binding.
* **External AI.** `runtime::coordinator::retry_task` (worker retry for stopped hand-offs). The `external_tasks` and `external_executions` tables hold attempts, outcomes and failures. Worker logs are on disk.
* **Project init.** `dx::project_init` is about 490 lines in the CLI crate. It mixes service logic (registration, people, BRD and knowledge ingestion, skills, manifest, client files, workspace registration) with text output and local credential storage.

## 2. Decisions for approval

| # | Decision | Recommendation | Alternative |
|---|---|---|---|
| W1 | **How restart, retry and reinitiation work** | **A derived run.** A new run of the same workflow definition is created from a source run and a chosen **restart stage**:<br>• States *before* the stage are carried over as they are: the same immutable artifact versions, outputs and quality results, recorded as carried from the source run.<br>• Gates before the stage are carried **only** if their approval is valid for the carried artifact hash. The engine records them as `carried_approvals: {gate → (source run, approval)}`, and `check_gate` and the completion guard accept them only through this verified lineage.<br>• If an earlier gate is not validly approved, that stage is the latest allowed restart point, so a mandatory approval can never be skipped.<br>• The restart stage and everything after it run again, producing new versions. They need new approvals. Earlier approved versions are never overwritten.<br>• The source run is never modified. If it is still active it is **cancelled** with the reason "superseded by run X", and audited.<br>• *Retry a failed stage* and *re-execute a stage* are restarts at that stage. *Reinitiate with new input* (for example a new BRD version) is a restart at the first stage with the new input.<br>• A **preview** (no writes) lists, per stage: carried, re-run or not reached; the artifacts and versions; which approvals stay valid and which must be repeated; and the effect on the workspace documents. Starting needs an explicit confirmation carrying the preview's fingerprint, so no stale confirmation is accepted. | Rewind the same run in place: reopen a terminal or active run at a stage and mark later data invalid. That mutates a run's history and breaks the "approval bound to run" invariant. |
| W2 | **Who the portal acts as for operations** (today: viewer = read, `--approver` = decisions, `--steward` = skill registration) | Explicit identities, as for decisions:<br>• `--operator <profile>` (workflow.start) for start, cancel, resume, restart and retry;<br>• `--admin <profile>` for registering a project, which creates the project and grants or creates people.<br>Without these flags the controls are hidden, and the API refuses with a hint. Backend authorization is unchanged, and every operation is audited as that identity. | The viewer (`--as`) performs operations. Fewer flags, but every portal session can then change state. |
| W3 | **Directory selection for "Register Existing Project"** | The server browses only under allowed roots:<br>• `--workspace-root <dir>` (repeatable);<br>• default: the parent folder of the EAEH repository (here `~/repo/test-harness`).<br>Paths are canonicalized, must stay inside a root, hidden folders are skipped, and symlinks that leave a root are refused. Detection is read-only:<br>• stack: Maven/Gradle → Java and Spring Boot, `package.json`, `pyproject.toml`, `go.mod`, `Cargo.toml` …;<br>• Git (`.git`, branch, remote);<br>• documents: BRD candidates, README, `docs/`, ADRs, standards;<br>• an existing EAEH manifest, which leads to *already registered* or *duplicate* handling. | Free-text path only, validated server-side (no browsing). |
| W4 | **Registration semantics** | Split `dx::project_init` into one service function, with no printing, called by both the CLI (which prints as today) and the portal. It writes what the CLI writes into the repository (`.eaeh/project.toml`, `CLAUDE.md`, `.mcp.json`, editor files: no secrets) and **never copies or moves source code**. People: existing principals are granted access. New people get tokens stored in the local credential store (as `--store-profiles`), never sent to the browser. | Portal registration without people management: existing principals only. |

## 3. Implementation outline (after approval)
1. **Parity inventory.** Every one of the 76 CLI operations is classified as Portal action, Portal read-only, CLI-only (admin or developer) or not applicable, with the reason. Implemented as `docs/implementation/EAEH-PORTAL-CLI-PARITY.md` and a test that parses the CLI command tree and fails if an operation is missing from the inventory.
2. **Engine (W1).**
   * Migration `0007` adds the `workflow_lineage` table (new run, source run, restart stage, reason, actor, carried states, carried approvals, preview fingerprint, created). The `WorkflowData` field `carried_approvals` is added, and existing runs stay valid.
   * `WorkflowEngine::restart_plan` (preview) and `restart` (execute) go through authorization (`workflow.start`), audit (`workflow.restart`), events and idempotency: one derived run per source run, restart stage and fingerprint.
   * `check_gate` accepts carried approvals through verified lineage.
3. **Operations API (`/v1`, shared with CLI and MCP where relevant).**
   * `POST /v1/workflows/{id}/resume`;
   * `POST /v1/workflows/{id}/restart/preview`;
   * `POST /v1/workflows/{id}/restart`;
   * `POST /v1/tasks/{id}/retry` (worker retry of a stopped hand-off);
   * the existing start and cancel;
   * `GET /v1/workflow-definitions/{id}/plan` (execution plan and readiness for a project);
   * `GET /v1/tasks/{id}` (detail: input, agent and skill versions, dependencies, attempts, hand-off, output artifacts, validation, error, log excerpt).
   * The CLI gains `eaeh workflow restart <RUN-ID> --from <stage> [--input …] [--yes]` (preview, then confirm) on the same engine functions.
4. **Project onboarding (W3, W4).** `GET /portal/fs?path=` (directory listing under the roots), `POST /portal/projects/inspect` (detection), `POST /portal/projects/register` (`--admin`). These are portal-process endpoints because they use local paths and the credential store; they call the shared service.
5. **Portal UI.**
   * Projects → **Register existing project** wizard: choose folder, detected information, edit, confirm.
   * Project → **Start workflow**: definitions from the backend, inputs (BRD and documents), execution plan with readiness, confirm.
   * Workflow page: operations bar (resume, cancel, restart from stage, retry) with the restart preview dialog; lineage links (restarted from / superseded by); task list with links.
   * New **Task** page.
   * Dashboard: running tasks, waiting-for-AI, recently executed tasks, filters (project, workflow, stage, task status, date), a per-workflow pending action or blocker, available operations.
   * Read-only pages for Knowledge sources and documents, Agents, Execution (workers, attempts, failures) and Integrations (execution mode, MCP and client configuration status, worker status).
6. **Security.**
   * Operations need the explicit identities (W2) plus backend authorization.
   * Paths are confined (W3).
   * Every state change goes through the engine and services, with audit. The portal never writes database records.
   * Logs are shown as redacted excerpts.

## 4. Tests (planned)
Mandatory scenarios 1–15 of the source document, using isolated installations. `simple-springboot-api-b` is used read-only: its runs, approvals and artifacts are not touched.
* **Rust:**
  * engine restart, carry and invalidation, idempotency, gate-bypass refusal, lineage, completion guard with carried approvals;
  * API and portal operations and authorization;
  * project inspection and registration with path confinement and duplicates;
  * parity inventory completeness.
* **Real binary end to end:**
  * register an existing Spring Boot folder through the portal;
  * start design;
  * approve requirements and HLD;
  * worker interruption and recovery;
  * restart from HLD after a requirements change;
  * CLI and portal state consistency.
* **Vitest** for the wizard, the operations, the restart preview, the task page and the dashboard filters.
* **`scripts/ci.sh`.**

## 5. Not in scope
* Phase 4–8 functionality.
* Production authentication: the portal stays local-only with no login.
* Administrative CLI operations kept CLI-only, listed with reasons in the inventory:
  * tokens, `admin bootstrap`, `init`;
  * plugin management;
  * knowledge ingestion and deletion;
  * `mcp`, `serve`, `auth`.
