# EAEH Governance Portal – Implementation Plan (supplementary UI enhancement)

| Field | Value |
|---|---|
| Status | **Approved 2026-11-14** (decisions D1–D4 below) |
| Date | 2026-11-14 |
| Source | `Approval-Portal.md` (UI amendment) |
| Scope | Visibility and approval interaction for Phases 1–3 only |
| Roadmap impact | None. `claude-instructions.md` phases, next steps, dependencies and acceptance criteria stay unchanged. |

## 1. What already exists (inspected)

| Capability needed by the portal | Existing implementation | Reuse |
|---|---|---|
| HTTP API, conventions | `eaeh-api` (Axum 0.8, `/v1`, bearer auth, `{"error":{code,message,request_id}}`, `limit`/`cursor` pagination, per-principal rate limit) | Reuse as-is |
| Projects | `GET /v1/projects` (filtered by principal's project access) | Reuse |
| Workflows | `GET /v1/workflows?project_id`, `GET /v1/workflows/{id}` (instance + transition history), `GET /v1/workflow-definitions` | Reuse |
| Workflow states | `InstanceStatus`: running, waiting_approval, **waiting_external** (external AI), succeeded, failed, cancelled, escalated; `WorkflowData` holds artifacts, approved gates, current approvals, quality results, failure | Reuse; map to UI statuses |
| Approvals | `GET /v1/approvals?project_id&status`, `GET /v1/approvals/{id}`, `POST /v1/approvals/{id}/decision` → `Harness::decide_approval` (role, SoD, author exclusion, artifact-hash check, audit, workflow resume) | Reuse – **the only decision path** |
| Documents | `GET /v1/artifacts/{id}?project_id` (metadata + content, `artifact.read`) | Reuse |
| Traceability | `GET /v1/traceability?project_id` | Reuse |
| Audit | `GET /v1/audit?after_seq` (ascending, tenant-scoped) | Extend (latest-first) |
| Identity | ADR-0024 local profile store; approval decisions are never auto-selected | Reuse |
| Tasks | `tasks` / `tasks_v4`, `external_tasks` tables; no task listing endpoint | **Gap** |

## 2. Proposed architecture

```
Browser (React SPA, 127.0.0.1 only)
   │  same-origin fetch /api/v1/*   (no token in the browser)
   ▼
eaeh portal  (new CLI command, in-process Harness – same SQLite DB, no new service)
   ├─ local-access guard: loopback bind, Host/Origin check, per-launch secret cookie
   ├─ identity injection: viewer profile token → Authorization header
   │                     approver profile token → only on POST …/decision
   ├─ serves portal/dist (static files)
   └─ eaeh_api::router  (unchanged authz, approval manager, audit, workflow engine)
```

* **No login, no new auth subsystem.** The portal process acts as identities the developer already stored with `eaeh auth login` (ADR-0024). Backend authorization runs unchanged on every request because requests still pass through `eaeh_api::router` with a real bearer token.
* **Why a launch secret:** without it, any local process – including the connected AI client in `external_ai` mode – could POST to `127.0.0.1` and approve using the stored approver token, contradicting §3.6 "Clients have no approval capability". The secret is generated per launch, printed once in the terminal URL, exchanged for an `HttpOnly; SameSite=Strict` cookie and never written to disk. It is not a login page; the portal opens directly on the dashboard.
* **No separate approval engine:** the UI posts `{decision, comments, artifact_hash}` to the existing endpoint; the backend rejects stale hashes, authors, wrong roles and agents.

### New/extended backend endpoints (read-only, minimal, in `eaeh-api`, bearer-authenticated like the rest)

| Endpoint | Purpose | Authorization |
|---|---|---|
| `GET /v1/dashboard` | Counts + active work + pending approvals across the caller's accessible projects (one request instead of N per project) | per project: `workflow.read` / `approval.read`; inaccessible projects skipped |
| `GET /v1/workflows/{id}/tasks` | Task executions (state, agent, status, start/update times, external hand-off status) | same as `GET /v1/workflows/{id}` |
| `GET /v1/audit?order=desc&limit=` | Latest-first recent activity | `audit.read` (unchanged) |

Exact shapes are fixed after reading the task/agent store code and documented in `EAEH-GOVERNANCE-PORTAL.md`. No schema migration. No write endpoints added.

### Frontend (`portal/` at repository root)

React 19, TypeScript, Vite, Tailwind CSS 4, shadcn/ui (Radix), React Router, TanStack Query (polling: 10 s dashboard, 5 s for an active workflow, paused when the tab is hidden), React Hook Form + Zod (decision form), Lucide. Zustand only if a shared client state appears (not expected). Exact versions pinned in `package.json` + lockfile.

Screens: Dashboard · Projects / Project detail · Workflows / Workflow detail (lifecycle built from the definition's states + transition history + `WorkflowData`) · Approvals / Approval review (document, metadata, version/hash, quality results, traceability, decision dialog with confirmation) · Activity / Audit (filter + search) · Settings (read-only: execution mode, active profiles, API version – only what exists).

Status mapping: running → Running; waiting_external → Waiting for AI; waiting_approval → Waiting for approval; succeeded → Completed; failed/escalated → Failed; cancelled → Cancelled.

## 3. Out of scope

Phases 4–8 functionality; authentication providers/login/OAuth/Keycloak; project creation/configuration; new database or migration; changes to `claude-instructions.md` roadmap content; production/multi-user exposure.

## 4. Testing

| Level | What | Tooling |
|---|---|---|
| Backend unit/API | new endpoints: authz, scoping, errors, pagination | `eaeh-api/tests/api.rs` (tower oneshot, real harness) |
| Portal server | loopback/Origin/secret guard, token injection, approver-only-on-decision, no token leakage, static serving | `eaeh-cli` tests |
| Integration (real data) | deterministic-mode design workflow → portal shows waiting_approval → decision via portal path → hash/audit preserved → engine resumes | `tests/tests/portal.rs` |
| Frontend | components, status mapping, empty/error states, decision form validation | Vitest + Testing Library |
| Browser E2E | dashboard without login, review + decide against a running `eaeh portal` with real data | Playwright (dev-only), D4 |
| Regression | `cargo test --workspace`, clippy, fitness tests, CLI + MCP tests | existing |

Automated tests use test approver identities as existing tests already do. In the live acceptance run, **you** make the approval decisions in the portal; I will not.

## 5. Deliverables

`portal/` frontend; `eaeh portal` command; endpoints above; ADR-0026 (portal local-access model); `docs/implementation/EAEH-GOVERNANCE-PORTAL.md`; short portal section in the user manual; evidence-based completion report `docs/implementation/stages/GOVERNANCE-PORTAL-COMPLETION.md`. No commits.

## 6. Risks

* A stored approver token used by a local server is a new attack surface → mitigated by D2; residual risk documented.
* Frontend toolchain (Node 22, npm) becomes a build prerequisite for the portal only; `cargo build` does not need it.
* SQLite single-writer shared with CLI/MCP (existing A-20) – the portal mostly reads.

## 7. Approved decisions (2026-11-14)

| # | Decision |
|---|---|
| D1 | Approver identity comes from a launch flag: `eaeh portal --as <viewer> --approver <approver-profile>`. Decisions are disabled when no approver is named. |
| D2 | Local guard: loopback bind, Host/Origin checks and a per-launch secret exchanged for an HttpOnly SameSite=Strict cookie. |
| D3 | `eaeh portal` serves `portal/dist` from disk (path configurable). Node is not needed for `cargo build`. |
| D4 | Playwright browser E2E (dev-only) runs against a real `eaeh portal`. |
