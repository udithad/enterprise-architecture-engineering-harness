# ADR-0031: Portal operations, project registration by location, and restart as a derived run

| Field | Value |
|---|---|
| Status | Accepted (plan `docs/implementation/stages/PORTAL-OPERATIONS-PLAN.md`, decisions W1–W4 approved 2026-11-17) |
| Date | 2026-11-17 |
| Amends | ADR-0026 §4. The portal can now operate workflows and register projects, not only read and decide. It also amends the engine rule "a failed run is final: start a new run". |
| Phase | Supplementary enhancement within the Phase 1–3 scope |

## Context
Routine work needed the CLI: registering a project, starting a workflow, resuming, retrying, cancelling. The engine could not continue a run from an earlier stage. A requirements or architecture change, or a failed stage, meant starting from scratch, with no link between the runs. Approvals bind to `"{run}:{gate}"` and the completion guard verifies that binding, so any rewind of a run in place would break the approval invariant.

## Decision
1. **Restart is a derived run (W1).** `WorkflowEngine::restart_plan` (preview, no writes) and `restart` (execute) create a **new** instance of the same definition from a source run and a restart stage.
   * **Before the stage:** states are carried. That means outputs, quality, the same immutable artifact versions, authors, visits and plans. Gates are carried **only** if their approval is valid for the carried artifact hash. They are recorded in `WorkflowData.carried_approvals {gate → (origin run, approval)}`. `check_gate`, and therefore the completion guard, accepts a carried approval only when its origin is an **ancestor** in `workflow_lineage`.
   * **Allowed stages:** the definition's main path (success and approved transitions) up to the first gate that is not validly approved. A mandatory approval can never be skipped, and no stage outside the definition can be chosen (`invalid_input`).
   * **From the stage on:** everything runs again in a new workspace, producing new artifact versions that need new approvals. Approved versions are never overwritten.
   * **New input** (for example a new BRD version) is allowed only from the first stage.
   * **The source run** is never modified. If it is active, it is cancelled as superseded (audit `workflow.superseded`).
   * **Retry a failed stage** and **re-execute a task** are restarts at that stage.
   * **Confirmation** carries the preview's `fingerprint`. A stale preview is refused (`conflict`). The same (source, stage, fingerprint) returns the existing derived run (`UNIQUE` in `workflow_lineage`), so a repeated confirmation never runs work twice.
   * **Audit:** `workflow.restart` on the new run. Migration `0007` adds `workflow_lineage`; existing runs are unchanged.
   * **Resume** re-drives a non-terminal run from its persisted state: it waits again for an approval or an AI hand-off, and does not dispatch twice. It needs `workflow.start` and is audited as `workflow.resume`, in the CLI and the portal alike.
2. **Explicit portal identities (W2).**
   * `--operator <profile>` (must permit `workflow.start`) is used for start, cancel, resume, restart preview and confirmation, and hand-off retry.
   * `--admin <profile>` (`principal.manage`) is used for project registration.
   * Without them the operations are hidden, and the portal refuses with `operator_not_configured` / `admin_not_configured`. The backend authorizes every call again.
   * Token, principal, plugin, knowledge-ingestion and standards operations remain outside the allowlist (`not_available_in_portal`).
3. **Register existing project (W3, W4).**
   * **Portal-process endpoints**, behind the same local guard and session secret as the rest of the portal:
     * `GET /portal/fs`: sub-folders only, no files, hidden entries skipped;
     * `POST /portal/projects/inspect`: read-only detection of stack, Git and documentation, and the registration state;
     * `POST /portal/projects/register`: `--admin` only.
   * **Path confinement:** paths are canonicalized and must stay under `--workspace-root` (default: the parent of the EAEH repository). Symlinks that leave a root are refused. Document paths must be relative and stay inside the project folder.
   * **Duplicates refused:** a folder already registered here (unless an update is confirmed), the manifest of another installation, and an existing project name.
   * **Shared service:** registration calls `dx::register_project`, the service behind `eaeh project init`, through the `PortalHost` trait. It writes the same in-place files, never copies source code, and keeps new people's tokens in the local credential store.
4. **Read models (`/v1`).** All of them are paginated or bounded, and authorized per project; foreign projects answer `not_found`.
   * `GET /v1/workflows/{id}/lineage`
   * `GET /v1/tasks/{id}`: task, input, output, stage, dependencies, agent and skills, artifact, validation, hand-off without its package, attempts.
   * `GET /v1/task-queue`: filters, keyset cursor.
   * `GET /v1/projects/{id}/plan`: the `workflow run` planner.
   * `GET /v1/executions`
   * `GET /v1/integrations`
   * Workflow-queue filters, `pending_action`, and `operations` on the workflow detail.

## Consequences
* Routine operations no longer need the CLI. CLI and portal call the same engine, coordinator and registration functions. The parity inventory (`docs/implementation/EAEH-PORTAL-CLI-PARITY.md`) is enforced by a test against the command tree.
* Restarted work is traceable both ways, and history is never rewritten.
* A restart costs a new run. Carried stages are not executed again; everything from the restart point is.
* The portal stays a local-development tool with no login (ADR-0026). The new identities widen what a local session can do, so each identity is opt-in.

## Alternatives considered
* **Rewind the same run in place.** Rejected: it mutates history and breaks run-bound approvals.
* **Operations as the viewer (`--as`).** Rejected: every portal session could change state.
* **Free-text path without browsing.** Rejected by W3: browsing under the allowed roots is easier and is confined the same way.
