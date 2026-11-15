# Governance Portal – project discovery defect: investigation and plan

| Field | Value |
|---|---|
| Status | IMPLEMENTED 2026-11-15 – P1–P4 as recommended; see docs/implementation/EAEH-GOVERNANCE-PORTAL-PROJECT-DISCOVERY-FIX.md |
| Date | 2026-11-15 |
| Scope | Supplementary fix within Phases 1–3 (Governance Portal, ADR-0026). No roadmap changes. |

## 1. Root cause (from evidence, backend first)

| Check | Result |
|---|---|
| Project registry, real installation `.eaeh-data/eaeh.db` | `simple-springboot-api-b` (`01a1db9e…`, tenant `01a1d860…`) exists. Workflow `01a1e022…` is `waiting_approval` at `approve_requirements`. Approval `01a1e026-8c7d…` is pending. |
| Backend API on the real installation, as the CLI's `developer` identity (read-only portal, port 18293) | `GET /v1/projects` returns all three projects, including `-b`. `/v1/dashboard` shows `-b` with `pending_approvals: 2` and `waiting_approval: 2`. `/v1/approval-queue` lists `01a1e026-8c7d-75d4-8aaa-027203e9b938 approve_requirements`. |
| The portal the user was looking at (`127.0.0.1:8090`, pid 55154, started 2026-11-15 22:26) | Its command line is `eaeh portal --as developer --approver approver --bind 127.0.0.1:8090 --static-dir …/portal/dist`. Its working directory is `target/demo-portal/simple-springboot-api`. Its **open database is `target/demo-portal/data/eaeh.db`**. This is exactly how `scripts/demo-portal.sh portal` starts it. |
| Demo database | Contains only `platform` and its own copy of a project named `simple-springboot-api` (deterministic mode). |

**Cause.** The CLI and the portal were attached to **different EAEH installations**:
* **CLI:** `EAEH_CONFIG=config/eaeh.toml` → `.eaeh-data`.
* **Portal:** the throw-away demo installation from `scripts/demo-portal.sh` (`EAEH_CONFIG=target/demo-portal/eaeh.toml`) → `target/demo-portal/data`.

The demo uses the **same default port (8090)** as `eaeh portal`, and holds a project with the **same name** as the real one. The wrong portal therefore looked plausible, and a real portal started later could not bind 8090.

The listing logic, authorization, tenant association, caching and polling are correct. Nothing in the product prevented or flagged the mismatch: the portal header's installation chip existed but was easy to miss, and the CLI never printed which installation it uses.

## 2. Decisions for approval

| # | Decision | Recommendation | Alternative |
|---|---|---|---|
| P1 | Prevent and flag installation mismatches | • **Startup:** `eaeh portal` reads the project manifest in the current folder. If that project is not registered in this installation, it prints a prominent warning.<br>• **Port in use:** the error names the cause (another portal, possibly the demo) and the fix.<br>• **UI:** a visible installation panel on Dashboard and Projects (data dir, config, execution mode), plus a "Not seeing a project?" explanation.<br>• **CLI:** `eaeh project list` and `eaeh workflow status` print the installation.<br>• **Demo:** `scripts/demo-portal.sh` moves to its **own default port 8190** and labels itself DEMO. | Refuse to start when the current folder's manifest belongs to another installation (blocks legitimate multi-project use). |
| P2 | Source of project metadata (description, stack, workspace) | Existence and access come from the **DB registry**, unchanged. Workspace path comes from `project_workspaces` (ADR-0027). Description, stack and workflows are read server-side from that workspace's committed `.eaeh/project.toml`, only when its project id matches. **No second registry, no copy.** | New migration copying manifest fields into the DB at `project init` (a duplicate source that drifts). |
| P3 | Project status and readiness | • **List:** a lifecycle status derived from real records: Ready / In progress / Waiting for approval / Blocked / Completed / Failed, plus current workflow and stage, pending approvals, last activity and workspace.<br>• **Detail:** the existing readiness service (`eaeh project readiness`) runs read-only. | Readiness on the list too (runs the full check for every project on every poll). |
| P4 | Acceptance step "approval works through the portal" for `simple-springboot-api-b` | I verify everything up to the decision against your real installation. **You** approve in the portal (approvals are human-only). I then verify the progression read-only. | – |

## 3. Implementation (after approval)
* **API:**
  * `/v1/dashboard` adds per-project `description`, `stack`, `workspace`, `status`, `current_workflow` (definition, status, stage), `last_activity`;
  * `/v1/projects/{id}/overview` adds `manifest`, `workspace`, `readiness`, `documents` (BRD path and the Requirements, HLD and LLD states from ADR-0027) and `audit` (project-scoped, if `audit.read`);
  * `/portal/session` adds the installation's config file path.
* **Portal UI:**
  * Projects table with the new columns;
  * Project detail adds project information, workspace, BRD, documents, readiness, active and historical workflows, pending approvals, artifacts with validation and audit;
  * empty states for projects without workflows;
  * prominent installation panel and mismatch help.
* **Refresh (already in place, verified by tests):**
  * dashboard and projects poll every 10 s;
  * lists every 15 s;
  * active workflows every 5 s;
  * on window focus;
  * every query is invalidated after a decision.
* **CLI and scripts:** the P1 messages and the demo port.

## 4. Tests (planned)
* **CLI-binary end to end** (`crates/eaeh-cli/tests/project_discovery.rs`, isolated temporary installation), using the real binary:
  * `eaeh init`, `admin bootstrap` and `project init`;
  * a running `eaeh portal`;
  * the project is persisted and returned by `/v1/projects` and the dashboard without a workflow (status Ready);
  * a **second** project initialized while the portal runs appears without a restart (no caching gap);
  * the session reports the same data dir as the CLI;
  * the mismatch warning is printed.
* **Harness level** (`tests/tests/portal.rs`):
  * a running workflow, then waiting for approval with stage and pending approval;
  * project detail with workflows, approvals and documents;
  * a decision updates the dashboard;
  * tenant and project isolation (another tenant's caller and a caller without access see neither the project nor its approvals).
* **Vitest:** projects table, status derivation, empty states, installation panel, refetch after invalidation.
* **Playwright** against an isolated demo installation.
* **Real-data acceptance:** `simple-springboot-api-b` through the API and UI. You make the approval.
