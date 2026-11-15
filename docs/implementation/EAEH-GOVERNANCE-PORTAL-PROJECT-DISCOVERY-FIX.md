# Governance Portal – project discovery fix (supplementary)

> **Supplementary document.** This is a fix within Phases 1–3 for the Governance Portal ([ADR-0026](../architecture/adr/ADR-0026-governance-portal-local-access-model.md)). It does **not** change:
> * the eight-phase roadmap;
> * any phase definition, dependency, acceptance criterion or next step in `claude-instructions.md` or `docs/implementation/ROADMAP.md`;
> * any existing architectural decision.
>
> Plan and approved decisions P1–P4: `docs/implementation/stages/PORTAL-PROJECT-DISCOVERY-PLAN.md`.

## 1. Root cause

`simple-springboot-api-b` was missing from the portal at `127.0.0.1:8090` because **that portal was attached to a different EAEH installation**. The listing code was not at fault.

| Evidence (2026-11-15) | Result |
|---|---|
| Registry of the real installation (`.eaeh-data/eaeh.db`, used by the CLI, workers and MCP servers via `EAEH_CONFIG=config/eaeh.toml`) | `simple-springboot-api-b` `01a1db9e…` exists. Workflow `01a1e022…` is `waiting_approval` at `approve_requirements`. Approval `01a1e026-8c7d…` is pending. |
| Backend API on that installation (read-only portal, `developer` identity) | `/v1/projects` and `/v1/dashboard` return the project. `/v1/approval-queue` returns `01a1e026-8c7d…`. |
| The portal on port 8090 (pid 55154) | Command line: `eaeh portal --as developer --approver approver --bind 127.0.0.1:8090 --static-dir …/portal/dist`. Working directory: `target/demo-portal/simple-springboot-api`. Open database: **`target/demo-portal/data/eaeh.db`**. This is how `scripts/demo-portal.sh` starts the throw-away **demo** installation (deterministic mode). |
| Demo database | Only `platform` and its own copy of a project named `simple-springboot-api`. |

**Why it looked plausible**
* The demo used the same default port (8090) as `eaeh portal`.
* It contains a project with the same name as a real one.
* Neither the portal nor the CLI said clearly which installation they used. The header's installation chip was easy to miss.

**Not the cause**
* Project persistence and the tenant association;
* the list service and authorization;
* the workflow-, approval- and artifact-to-project associations;
* TanStack Query caching and refresh;
* filtering.

The isolated end-to-end test (§7) verifies each of these.

## 2. Project registration architecture (unchanged)

```
eaeh project init  ──▶ IdentityService.create_project / ensure_project ──▶ projects (DB)  ← authoritative registry
                   ──▶ .eaeh/project.toml (repo manifest: description, stack, BRD, roles, workflows)
                   ──▶ Materializer.register_workspace ──▶ project_workspaces (DB, ADR-0027)
eaeh portal / eaeh serve / CLI / MCP / worker ──▶ the same Harness on the configured data_dir
```

* **One registry.** The `projects` table of the configured installation, filtered by the caller's tenant and project access. The portal adds no registry and no copy.
* **Project metadata.** Description, stack, BRD, roles and registered workflows are read server-side from the registered workspace's committed manifest, and only if it declares this project id (decision P2). If no workspace is registered, or the manifest belongs to another project, the reason is returned as `manifest_issue` and nothing is guessed.
* **Portal creation of projects.** Not added (§9 of the request): registration stays `eaeh project init`, which is idempotent and validated. The portal is read- and decide-only (ADR-0026 allowlist).

## 3. Project listing API

These are read-only; authorization is unchanged (`workflow.read`, `approval.read`, `audit.read`, project access).

| Endpoint | New fields |
|---|---|
| `GET /v1/dashboard`, per project | `status`, `current_workflow`, `last_activity`, `description`, `stack`, `workspace`, `registered_workflows`, `manifest_issue` |
| `GET /v1/projects/{id}/overview` | `workspace`, `manifest`, `manifest_issue`, `readiness` (the read-only check of `eaeh project readiness`), `current_workflow`, `status`, `documents` (Requirements / HLD / LLD of the latest design run, ADR-0027 states), `audit` (if `audit.read`) |
| `GET /portal/session` | `installation.label` (e.g. `DEMO – …`) next to `data_dir` and `config_dir` |

`current_workflow` is `{id, definition_id, status, state, blocked, updated_at, created_at}`.

**Project status** is derived from the project's workflow records:

| Status | When |
|---|---|
| `ready` | registered, no workflow yet |
| `in_progress` | a workflow is running or waiting for the AI client |
| `waiting_approval` | a workflow waits at an approval gate |
| `blocked` | an approved gate could not be materialized (ADR-0027) |
| `completed` | the latest workflow succeeded |
| `failed` | the latest workflow failed |
| `cancelled` | the latest workflow was cancelled |

A project is listed in every state, with or without workflows.

## 4. Portal integration

* **Projects page.** Every project of this installation the identity can access, with:
  * name, description and id;
  * stack and registered workflows;
  * workspace path;
  * status badge;
  * current workflow and stage (linked);
  * pending approvals;
  * last activity.

  Sorted by last activity, searchable by name, id, description and stack.
* **Project detail.**
  * project information: id, status, current workflow, stack, workspace and its registration, BRD, registered workflows, roles, execution mode, constraints;
  * readiness, with blocking items and fixes;
  * documents in the workspace (Requirements / HLD / LLD, ADR-0027);
  * tabs: Workflows (history and active), Approvals, Artifacts (version, hash, gate approval = validation status), Recent activity, Audit (if permitted).
  * A project without workflows shows *"No workflows in this project yet"* with the start command for its workspace.
* **Installation panel** on the Dashboard, the Projects page and each project page shows:
  * the data dir, configuration and execution mode;
  * a prominent amber **DEMO** label for the demo installation;
  * the number of visible projects;
  * **"Not seeing a project?"**: how to compare with `eaeh project list` / `eaeh workflow status`, that the demo is separate, that access is per project, and the refresh interval.
* **Header.** A **DEMO** badge.

## 5. Persistence configuration (decision P1)

| Where | Behaviour |
|---|---|
| `eaeh portal` startup | Prints `data dir`, `config dir` and an `installation:` label when one is set. If the project manifest of the **current folder** is not registered in this installation, it prints a ⚠ warning with the folder project, the portal's data dir and the fix. |
| `eaeh portal`, port in use | `port 8090 is already in use – most likely another eaeh portal (scripts/demo-portal.sh runs one on its own DEMO installation). Find it with lsof …, stop it, or … --bind 127.0.0.1:<port>`. |
| `eaeh project list` | JSON includes `installation {data_dir, config_dir}`. |
| `eaeh workflow status` (and MCP status) | A line `installation: <data dir>` and JSON `installation.data_dir`. |
| `scripts/demo-portal.sh` | Default port **8190** (no longer 8090). Sets `EAEH_INSTALLATION_LABEL="DEMO – <dir>"`, so the banner, header and panels say DEMO. |

## 6. Refresh behaviour (already present; now covered by tests)

| What | Refresh |
|---|---|
| Dashboard and Projects (one read model) | Polled every 10 s |
| Lists | Every 15 s |
| Active workflows | Every 5 s |
| On window focus | Refetch |
| In background tabs | Polling pauses |
| After a decision | `invalidateQueries()` refreshes everything |

A project registered from the CLI therefore appears within about 10 s, and decisions show immediately. The engine resumes in the portal process, and the next poll shows the new stage.

## 7. Tests and evidence

**Automated** (results in §8 of the final report):

| Test | Covers (request §11) |
|---|---|
| `crates/eaeh-cli/tests/project_discovery.rs` – `cli_registered_projects_are_discoverable_in_every_state_without_restarting_the_portal` (real binary, isolated installation) | 1 `project init`; 2 persisted (`project list`); 3 `/v1/projects`; 5 listed without a workflow (`ready`, workspace, stack, description, workflows, readiness not ready); 6 and 7 a design run started from another CLI process shows as `waiting_approval` at `approve_requirements` with its pending approval; 8 overview lists the workflow and approval; 9 the pending approval; 10 the requirements document is opened and approved **through the portal**, and the dashboard follows to `approve_hld` with Requirements `verified` in the workspace; 11 and 14 a project registered while the portal runs appears without a restart; 12 the portal session's data dir equals the CLI's; a port collision is explained |
| `…::a_portal_on_another_installation_warns_that_the_folder_project_is_not_there` | The mismatch warning names the folder project and the portal's data dir; the project is not shown; the DEMO label reaches the banner |
| `crates/eaeh-api/tests/api.rs` – `portal_read_models_are_authenticated_and_project_scoped` (extended) | 13 Project isolation: the outsider's `/v1/projects` and dashboard list only their own project (`ready`, no current workflow) with no trace of the other project's workflow; all read models require authentication; foreign resources return `not_found` |
| `portal/src/test/projects.test.tsx` (4 tests) | 4 Rows with description, id, stack, workspace, status, current workflow (link), pending approvals; a project without workflows shown as Ready; the installation panel and its help; the DEMO label; 14 a project added later appears after the next refresh (no stale cache); status mapping |

**Tenant isolation.** Every listing filters by the caller's tenant (`identity.projects(p.tenant)`, `Scope::new(p.tenant, …)`). A second tenant cannot be created through the current APIs (multi-tenant administration is Phase 8), so tenant separation is verified by code inspection plus the project-isolation test, and is not exercised with two tenants.

**Real-data acceptance (`simple-springboot-api-b`).** Captured through a read-only portal on the real installation, before the approval:

| Field | Value |
|---|---|
| session `installation.data_dir` | `…/enterprise-architecture-engineering-harness/.eaeh-data` (same as the CLI) |
| `/v1/projects` | `simple-springboot-api`, `simple-springboot-api-a`, `simple-springboot-api-b` |
| dashboard row `-b` | `status: waiting_approval`, `pending_approvals: 2`, `stack: Java 21 · Spring Boot 3.x · Maven`, `workspace: /Users/…/simple-springboot-api-b` |
| `current_workflow` | `01a1e022-b374-7690-af21-5fcc94f3ae79` `engineering.design` `waiting_approval` `approve_requirements` |
| pending approvals | `01a1e026-8c7d-75d4-8aaa-027203e9b938` (approve_requirements), `01a1dfd9-…` (approve_lld of the earlier run `01a1dba5…`) |
| overview | readiness `ready: true, blocking: 0`; documents: requirements `awaiting_approval` |
| approval document `01a1e026…` | artifact `01a1e026-8c61-7154-82fc-099e19aae5fc`; files `analysis.json`, `requirements.json`, `requirements.md` (content readable) |

The **approval itself is made by the user** (approvals are human-only; P4). The progression to HLD and LLD is then verified read-only and recorded in the final report.

## 8. Authorization
* No login was added (ADR-0026 scope), and backend authorization is unchanged.
* The new fields are returned only for projects the identity can access. Metadata needs `workflow.read` or `approval.read` on the project, and audit needs `audit.read`.
* The manifest is read server-side only from the registered root and only when it declares the same project id. The browser never supplies paths.
* Restricted projects show only their name and id.

## 9. Known limitations
* **The portal cannot know which installation you meant.** It warns when the current folder's project is missing, labels the demo, and shows its data dir. Starting it with the right configuration remains your responsibility.
* **Project metadata comes from the repository manifest.** If the workspace is not registered (projects bootstrapped before ADR-0027 that have not run `eaeh workflow run` / `eaeh artifact sync` since), the description and stack are shown as unavailable, with the reason.
* **The project audit tab** shows the latest 50 entries of that project, taken from the most recent 400 entries of the tenant.
* **Refresh is polling-based** (local SQLite); there is no server push.
