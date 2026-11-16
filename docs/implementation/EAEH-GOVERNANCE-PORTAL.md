# EAEH Governance Portal (supplementary UI enhancement)

> **Supplementary document.** This adds a web UI to the existing roadmap. It does **not** change any phase definition, dependency, acceptance criterion or next step in `claude-instructions.md` or `docs/implementation/ROADMAP.md`. Scope: visibility and approval interaction for Phases 1–3 only.
>
> Source request: `Approval-Portal.md`. Plan and approved decisions: `docs/implementation/stages/GOVERNANCE-PORTAL-PLAN.md`. Architecture decision: [ADR-0026](../architecture/adr/ADR-0026-governance-portal-local-access-model.md).

> ⚠️ **No authentication. Trusted local development only.** The portal has no login. It acts as identities stored on this computer and listens on a loopback address only. Do not expose it to other machines, users or tenants. Authentication (enterprise identity, per-user sessions) **must** be introduced before the portal is used beyond a trusted local development environment.

## 1. Architecture

```
Browser (React SPA)                      eaeh portal  (one local process)
──────────────────                       ──────────────────────────────────────────────
GET  /, /assets/*      ───────────────▶  static files from portal/dist (SPA fallback)
POST /portal/session   {launch}  ─────▶  launch secret ⇒ HttpOnly SameSite=Strict cookie
GET  /portal/session            ──────▶  who the portal acts as (never the tokens)
GET  /api/v1/…  (cookie)        ──────▶  local guard ─▶ allowlist ─▶ inject viewer token ─┐
POST /api/v1/approvals/{id}/decision ─▶  local guard ─▶ allowlist ─▶ inject approver token ┤
                                                                                          ▼
                                         unchanged eaeh-api /v1 router (bearer auth, authz,
                                         rate limit, errors) ─▶ Harness (in-process)
                                           ├─ ApprovalManager / WorkflowEngine (decisions,
                                           │  hash check, SoD, audit, workflow resume)
                                           └─ read models (dashboard, queues, details)
                                         same SQLite database as the CLI and MCP server
```

* **No separate approval engine, service or database.** Decisions use `Harness::decide_approval`, the same path as `eaeh review` / `eaeh approval decide`. After an approval the existing engine resumes the workflow (`spawn_drive`).
* **Backend authoritative.** The UI never writes records. Role, separation of duties, author exclusion, artifact hash and the "already decided" check are all enforced by the approval service. The UI's pre-checks only explain what the service will refuse.
* **Local guard** ([ADR-0026](../architecture/adr/ADR-0026-governance-portal-local-access-model.md)):
  * loopback bind only;
  * `Host` check (DNS rebinding);
  * `Origin` check on writes;
  * per-launch 256-bit secret in an `HttpOnly; SameSite=Strict` cookie;
  * CSP `default-src 'self'`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `nosniff`, `no-store` on API responses.
* **Allowlist.**
  * GET: health, me, dashboard, activity, approval-queue, workflow-queue, workflows (list/one/detail), workflow-definitions, approvals (list/one/document), artifacts, projects (list/overview), audit, audit/verify, traceability, agents.
  * POST: `/v1/approvals/{id}/decision` only, and only for **workflow gates**.
  * Everything else returns `403 not_available_in_portal`.
* The portal does **not** start background recovery or dispatch (`eaeh serve` does). Starting it next to CLI workers therefore never interrupts their tasks.

## 2. Technology stack

| Layer | Choice (pinned exactly in `portal/package.json`, with `package-lock.json`) |
|---|---|
| UI | React 19.3.1, TypeScript 6.0.3, Vite 8.3.5 (`@vitejs/plugin-react` 6.1.2) |
| Styling | Tailwind CSS 4.3.3 (`@tailwindcss/vite`), neutral slate palette, shadcn/ui-style components on Radix UI primitives (alert-dialog, select, tabs, label, separator, slot, tooltip) |
| Routing / data | React Router 7.18.5, TanStack Query 5.105.0 (polling) |
| Forms | React Hook Form 7.91.0 and Zod 4.6.7 (`@hookform/resolvers` 5.9.2), used for the decision form |
| Documents | react-markdown 10.1.0 and remark-gfm 4.0.1 (raw HTML disabled) |
| Icons | lucide-react 1.65.0 |
| Tests | Vitest 5.1.0, Testing Library, jsdom 29.1.1, Playwright 1.64.1 |
| Backend | Existing Rust crates; Axum 0.8 (already used by `eaeh-api`); **no new Rust dependencies** |

Zustand is not used: no shared client state beyond server state, which TanStack Query holds. Version notes:
* Vitest 5.1.0: Vitest 4.1.x and 5.0.x crash npm 10.9.2's resolver.
* jsdom 29: jsdom 30 needs Node ≥ 22.22.
* Node ≥ 22.12 is required (`engines`, `engine-strict`).

## 3. Frontend structure (`portal/`)

```
portal/
  index.html, vite.config.ts, tsconfig.json, playwright.config.ts, package.json(+lock)
  src/main.tsx                 QueryClient, SessionGate, router
  src/App.tsx                  routes (dashboard eager, other screens code-split)
  src/lib/api.ts               same-origin client, ApiError, describeError (hints)
  src/lib/types.ts             API shapes
  src/lib/status.ts            status → label/tone/"waiting on" (Running, Waiting for AI,
                               Waiting for approval, Completed, Failed, Cancelled)
  src/lib/lifecycle.ts         stages from the workflow definition + history + tasks
  src/lib/format.ts            dates, durations, names
  src/hooks/queries.ts         TanStack Query hooks and polling intervals
  src/components/              AppShell (sidebar), SessionGate, StatusBadge, StatCard,
                               ActivityList, DocumentViewer, DecisionPanel, States
                               (loading/empty/error), ProjectFilter, ui/* primitives
  src/pages/                   Dashboard, Projects, ProjectDetail, Workflows, WorkflowDetail,
                               Approvals, ApprovalReview, Activity (+Audit), Settings, NotFound
  src/test/                    Vitest suites
  e2e/                         Playwright suites (real portal, real data)
```

## 4. Backend integration

| File | Change |
|---|---|
| `crates/eaeh-api/src/views.rs` (new) | Read models composed from existing services, project by project, with existing permissions |
| `crates/eaeh-api/src/portal.rs` (new) | Portal application: guard, session, allowlist, identity injection, static files |
| `crates/eaeh-api/src/routes.rs` | Mounts the read models; `GET /v1/audit` gains `order=desc` and `before_seq`; three helpers made `pub(crate)` |
| `crates/eaeh-workflow/src/engine.rs` | Read methods `tasks_for`, `active_tasks`, `list_active`, `status_counts`, `recent_activity` and `gate_checks`; types `TaskSummary`, `ActivityItem`, `GateCheck`; **completion guard** on successful terminals (§7a) |
| `crates/eaeh-store/src/audit.rs` | `audit_recent` (newest first), `audit_by_resource` |
| `crates/eaeh-cli/src/portal.rs` (new), `main.rs` | `eaeh portal`: resolves stored profiles (ADR-0024) and runs the server |

**No database or persistence change:** no migration, no new table, and no write path.

## 5. API contracts (new or extended; all `Authorization: Bearer`, errors `{"error":{code,message,request_id}}`)

| Endpoint | Authorization | Returns |
|---|---|---|
| `GET /v1/dashboard` | per project: `workflow.read`, `approval.read` (projects without either are `restricted`) | `summary` {projects, in_progress, executing_tasks, waiting_external, waiting_approval, pending_approvals, completed, failed_or_blocked}, `projects[]`, `active[]` {project, workflow, tasks}, `pending_approvals[]`, `activity[]` (25), `execution_mode`, `generated_at` |
| `GET /v1/activity?project_id&limit≤100` | `workflow.read` | workflow starts and transitions, classified as `workflow_started`, `task_completed`, `artifact_generated`, `approval_requested`, `approval_granted`, `approval_rejected`, `changes_requested`, `stage_failed`, `workflow_completed`, `workflow_failed`, `workflow_cancelled` |
| `GET /v1/approval-queue?status=pending\|all\|approved\|…&project_id&limit≤200` | `approval.read` | approvals with project, document type (Requirements/HLD/LLD), artifact name, version and hash, required role, authors, workflow |
| `GET /v1/workflow-queue?status=active\|all&project_id&limit≤100` | `workflow.read` | instances (agent outputs omitted) |
| `GET /v1/workflows/{id}/detail` | `workflow.read` (else `not_found`) | instance, definition, transition history, task executions (state, kind, task kind/tool, agent `id@version`, status, attempt, error, times), approvals of the instance |
| `GET /v1/approvals/{id}/document` | `approval.read` (else `not_found`) and `artifact.read` | approval, document (manifest artifact plus every bundle file with content, **each file's hash checked against the manifest**), workflow context (instance, previous requests at the gate, trace links of the instance) |
| `GET /v1/projects/{id}/overview` | access to the project (else `not_found`) and `workflow.read` or `approval.read` | project (name, domain), status (active/idle/no_workflows), counts, workflows, engineering artifacts (latest version per name, reviewing gate, approved?), activity, approvals |
| `GET /v1/audit?order=desc&before_seq&limit≤500` | `audit.read` (unchanged) | newest-first audit records; `next_cursor` = last seq |

Reused unchanged:
* `POST /v1/approvals/{id}/decision` `{decision: approve|request_changes|reject, comments, artifact_hash}`;
* `GET /v1/workflow-definitions`, `/v1/projects`, `/v1/me`.

Projects have no description or status column in EAEH. The portal shows the knowledge domain and derives the status from workflows; it invents neither.

## 6. Screens and dashboard functionality

* **Dashboard** (first screen, no login):
  * overview cards: projects, workflows in progress (split into waiting for AI and waiting for approval), tasks executing, pending approvals, completed, failed or blocked;
  * in-progress table: project, workflow name and id, stage with "Stage k of n", current task, agent, status, waiting on (External AI / Human approval / System processing), started, updated, duration. Rows open the workflow;
  * pending approvals with a direct **Review** action;
  * recent activity.

  Refreshes every 10 s; polling pauses while the tab is hidden.
* **Workflow detail**: the lifecycle is built from the instance's own definition. For `engineering.design` that is BRD → Analyse → Refine → Render requirements → Approve requirements → HLD → Render HLD → Approve HLD → LLD → Render LLD → Approve LLD.
  * Each stage shows its status: completed, in progress, waiting for AI, waiting for approval, failed, rejected or not started, plus its revision count.
  * Selecting a stage shows its timestamps, artifact (version and hash), tasks with agents and errors, and the gate approval with a link to the document.
  * The page also shows the transition history, a failure banner and the external-AI hand-off banner.
* **Approvals**: filters for status, project and document type, plus search.
* **Approval review**:
  * metadata: project, workflow, status, approver role, artifact, version, full hash with copy, requester, authors who cannot approve, size;
  * Document tab: every bundle file, Markdown rendered or as source;
  * Validation & policy: quality-gate results;
  * Traceability: links of the run, filterable;
  * History: earlier requests at this gate, with decisions and comments.
* **Decision** (workflow gates):
  * Approve, Request changes and Reject. Comments are required for the last two (Zod).
  * A confirmation dialog shows the project, gate, full hash, approver, comments and consequence.
  * Before submitting, the UI re-reads the request and refuses if its status or hash changed.
  * After submitting, the result is shown with success or failure feedback and the service's message.
  * Without `--approver` the panel is read-only and explains how to restart.
* **Projects / Project detail**: counts per project; tabs for workflows, approvals, engineering artifacts and activity.
* **Activity / Audit**: workflow activity (filter by project and event type). The audit log is newest first with paging and a filter. It needs `audit.read`; otherwise the page says which role holds it.
* **Settings** (read-only): viewing and approver identities (profile, principal, roles, projects), execution mode, API version, security notice.

Every page has loading, empty and error states (status, code, message, request id and a corrective hint). There are no decorative controls.

## 7. Approval lifecycle through the portal

1. A workflow reaches an approval gate. The engine creates the request, bound to the artifact hash, and the workflow becomes `waiting_approval`.
2. The dashboard and Approvals pages show it.
3. The reviewer opens it and reads the document. Every bundle file is hash-checked on the server.
4. The reviewer chooses a decision, adds comments and confirms. The UI re-reads the request and stops if it changed.
5. The portal injects the `--approver` token and forwards the request to `POST /v1/approvals/{id}/decision`.
6. `Harness::decide_approval` → `WorkflowEngine::record_decision` checks the human identity, role, separation of duties and authors, the artifact hash and that the request is still pending. It records the approver, role, time, decision, comments and hash, and writes the audit entry.
7. The existing engine resumes. Approved: the next stage runs. Changes requested: revision, bounded by the gate's limit (ADR-0020). Rejected: the rejection path.

## 7a. Completion guard and generation provenance (added 2026-11-14 after acceptance review)

A reviewer saw the HLD and LLD stages finish within a fraction of a second and asked whether the approvals had really happened. The records showed three genuine human decisions, each bound to its hash. The speed came from **deterministic mode**: agent stages replay recorded responses, which is not AI. The review still exposed one real gap and one visibility problem, and both are fixed:

* **Engine completion guard** (`WorkflowEngine`, applies to every workflow). Before a workflow is recorded as `succeeded`, every approval gate from which the success end is reachable must have a granted approval that:
  * is `approved`, with a recorded `approve` decision;
  * belongs to this instance and project;
  * was decided by an approver who is not an author;
  * is bound to the reviewed hash, and that hash is the **current** version of the reviewed artifact.

  Otherwise the workflow ends `failed` with `completion guard: …`. Each check is audited as `workflow.completion_check`, `succeeded` or `denied`, with the gates and any violations. Previously, reaching the success end relied only on the transition graph. Work stages were already guarded by `requires_approval_of`. All shipped definitions were checked: every gate that leads to success lies on every success path, so legitimate runs are unaffected.
* **Gate-by-gate verification in the portal.** `GET /v1/workflows/{id}/detail` returns `governance`:
  * per-gate checks (approver, role, decision time, approved hash against the current artifact hash, issues), computed by the same engine function as the guard (`gate_checks`);
  * the recorded completion check;
  * `completion_verified`.

  The workflow page shows a **Governance verification** table and a "Verified: n/n gates approved" or "Completion not verified" seal. Runs that finished before the guard existed show their gate table and state that no completion check was recorded.
* **Generation provenance.** Artifact provenance is classified as:
  * `external_ai`: `external:<client>`, i.e. Claude Code or Copilot via MCP;
  * `deterministic_replay`: `replay/…`, recorded fixture, **not AI**;
  * `direct_provider`: an EAEH model provider;
  * `platform`: rendering or tool steps with no model.

  The workflow and review pages show a badge per artifact and a banner when a run used recorded responses. The header marks deterministic mode. In `external_ai` mode the same stages show **Waiting for AI** until the client submits a validated result. That usually takes minutes, which is the timing you would expect from Claude via MCP.

Tests:
* `completion_guard_refuses_success_without_a_valid_gate_approval`: a flawed definition whose rejected gate leads to `done` ends `failed`, with a denied audit record.
* `completion_guard_records_success_for_a_properly_approved_run`.
* `completed_design_run_is_verified_gate_by_gate_with_generation_provenance`.
* Vitest `governance.test.tsx`.
* Playwright "a completed design run shows gate-by-gate approval evidence…".

## 7b. Documents in the project workspace (added 2026-11-15)

Once a document is approved, the engine writes that version into the application repository under `docs/architecture/{requirements,hld,lld}/` (ADR-0027, [EAEH-ARTIFACT-MATERIALIZATION.md](EAEH-ARTIFACT-MATERIALIZATION.md)). The portal shows the result:

* **Workflow page:** the **Documents in the project workspace** card shows:
  * the workspace root;
  * per document: name and type, version with hash, approval, workspace path, materialization state with explanation, and last written time.

  A red **Blocked** banner appears, and the status badge reads `Blocked`, when an approved document could not be written.
* **Review page:** the **Project workspace** card says where this version will be written once approved, and its current state.

States come from `GET /v1/workflows/{id}/detail` (`documents`, `workspace`) and `GET /v1/approvals/{id}/document` (`workflow.workspace_document`). The backend re-hashes the files for each response, so **"verified" is shown only if every file on disk matches the approved hash at that moment**. The portal performs no writes: reconciliation is `eaeh artifact sync` in the project repository.

## 7c. Project discovery and installation visibility (added 2026-11-15)

Projects appear in every lifecycle state. Each row shows the derived status, current workflow and stage, stack, workspace, pending approvals and last activity. The project page shows the manifest, readiness, documents and audit. An installation panel and startup warnings make a CLI/portal installation mismatch visible, and the demo has its own port (8190) and a DEMO label. Details: [EAEH-GOVERNANCE-PORTAL-PROJECT-DISCOVERY-FIX.md](EAEH-GOVERNANCE-PORTAL-PROJECT-DISCOVERY-FIX.md).

## 7d. Skills (added 2026-11-16)

The **Skills** page lists every skill file (Markdown or JSON) with its layers and status.
* With `--steward <profile>`, **Register skill files** registers the unregistered files as pending approval (`POST /v1/skills/sync`, the same service as `eaeh skill sync`).
* Skill approvals are reviewed with their full definition and digest, and decided by the `--approver` identity.

Mandatory standards stay CLI-only. See [ADR-0028](../architecture/adr/ADR-0028-markdown-skills-and-portal-skill-governance.md) and [EAEH-MARKDOWN-SKILLS.md](EAEH-MARKDOWN-SKILLS.md).

## 8. Local startup

Prerequisites: Rust toolchain (repository pin), Node ≥ 22.12 and npm.

```bash
# build the UI once (or after UI changes)
npm --prefix portal ci && npm --prefix portal run build
# from a project folder with stored profiles (eaeh auth status)
eaeh portal --as developer --approver approver --open   # opens http://127.0.0.1:8090/?launch=…
eaeh portal --as developer                              # read-only
eaeh portal --bind 127.0.0.1:18190 --static-dir /path/to/portal/dist
# without --static-dir: ./portal/dist, else <repo>/portal/dist next to the configuration
# keep the terminal open – the portal is a server; Ctrl-C stops it
```

Open the printed link (it contains the one-time launch secret; do not share it). Restarting the portal invalidates the old link.

**Demo with real data** (deterministic mode, recorded responses, not AI; throw-away data under `target/demo-portal`):

> The demo is a **separate installation**: its own configuration (`target/demo-portal/eaeh.toml`), its own database and its own default port **8190**, and the UI labels it **DEMO**. Projects you create with the CLI on your real installation (`config/eaeh.toml`) never appear in it. To see your own projects, run `eaeh portal …` in your shell with your usual `EAEH_CONFIG`, on port 8090.

```bash
scripts/demo-portal.sh prepare          # installation, project, skills registered
scripts/demo-portal.sh approve-skills   # you, as the human approver (per-item confirmation)
scripts/demo-portal.sh start            # design workflow to the Requirements gate + portal
```

**UI development** with hot reload:

```bash
eaeh portal --as developer --approver approver --dev-origin http://localhost:5173
npm --prefix portal run dev             # http://localhost:5173/?launch=<secret from the portal output>
```

## 9. Testing

| Suite | Command | Covers |
|---|---|---|
| Portal acceptance (Rust, real harness and the real `engineering.design` workflow; model = labelled `ScriptedModelProvider`) | `cargo test -p eaeh-acceptance --test portal` | guard (Host, Origin, secret, cookie flags); allowlist; client credential ignored; no token in any response; dashboard counts, active work, pending Requirements, activity; workflow detail with tasks and agents; queues; project overview; document with hash and quality; read-only refusal; backend refusal (role, stale hash); request changes and approve through the approval service with hash and audit preserved; engine resumes to the HLD gate; second decision refused; empty installation; error contract; static UI with SPA fallback, security headers, no traversal; skills not decidable through the portal |
| API contract | `cargo test -p eaeh-api --test api portal_read_models` | read models need authentication, are project-scoped (outsider sees nothing, `not_found`), validate input; `audit?order=desc` keeps `audit.read` and pages correctly |
| Frontend unit and component | `npm --prefix portal test` | status mapping (Waiting for AI ≠ Waiting for approval), lifecycle from the shipped definition, decision rules, comments required, confirmation, payload carries the reviewed hash, stale-document refusal, backend refusal shown, SoD pre-check, read-only mode, dashboard rendering, empty states, error messages with hints, launch-secret exchange, no-session screen without a login form |
| Browser E2E (Playwright, real `eaeh portal` and real data) | see below | no login screen and secret removed from the URL; real in-progress work and Requirements approval; document readable; validation; decision validation and confirmation (cancelled); lifecycle with tasks and agents; projects, artifacts, activity, audit permission, settings; errors and empty states; no data without the secret |
| Regression | `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` | all existing suites, including architecture fitness, CLI, DX and MCP |

Browser E2E:

```bash
scripts/demo-portal.sh prepare && scripts/demo-portal.sh approve-skills
EAEH_PORTAL_PORT=18190 scripts/demo-portal.sh start > target/portal.url &   # keeps running
cd portal
EAEH_PORTAL_URL="$(head -1 ../target/portal.url)" npm run e2e
#   add EAEH_E2E_CHANNEL=chrome to use the installed Google Chrome when
#   `npx playwright install chromium` cannot download the bundled browser
# after YOU approved the Requirements gate in the portal:
EAEH_PORTAL_URL="…" EAEH_E2E_AFTER_REQUIREMENTS_APPROVAL=1 npm run e2e -- after-decision
```

The browser suites never confirm a decision; workflow gate decisions in real data are made by a human. Automated decision coverage lives in the Rust suites, using fixture identities in throw-away installations.

## 10. Authentication limitation and security implications

* **No login and no per-user identity in the browser.** Everyone who can use the open browser session acts as the portal's viewer and approver identities.
* **Mitigations:**
  * loopback only;
  * Host and Origin checks;
  * per-launch secret cookie (`HttpOnly`, `SameSite=Strict`);
  * tokens never sent to the browser;
  * client `Authorization` headers discarded;
  * allowlist;
  * decisions only as the explicit `--approver`;
  * unchanged backend authorization, SoD, hash checks and audit.
* **Residual risks:**
  * A process that reads the portal's stdout (the launch URL) or the browser's cookies can act within the session. **Start the portal from your own terminal, never through an AI client's shell tool.**
  * The secret is valid for the life of the process.
  * Other users of the same OS account and browser profile share the session.
  * The approver token stays in memory while the process runs.
* Requirement: authentication must be introduced before exposing the portal beyond trusted local development.

## 11. Scope limitations (Phases 1–3)

* Visibility of Phase 1 (workflows, tasks, agents, audit) and Phase 3 (Requirements, HLD and LLD documents, gates, traceability). Phase 2 is visible indirectly: skills in task packages, standards in quality results.
* Decisions only for workflow document gates. Skills, standards and knowledge approvals stay in the CLI, which shows their full content.
* No project creation or configuration, no workflow start or cancel, no token or principal management.
* **Nothing from Phases 4–8 was implemented or changed.** Delivery workflows (`engineering.delivery*`) appear only as recorded data if they exist. The portal adds no delivery, testing, Git/CI, release or scale capability.
* Polling, not server push. Lists are capped (100–200 items per project per view).

## 12. Future considerations

* Enterprise authentication (OIDC) and per-user sessions before any shared deployment, replacing identity injection.
* Server-sent events from the existing outbox instead of polling.
* Portal review for skills and standards with full content rendering.
* Side-by-side diff between artifact versions for revisions.
* HTTP equivalents for activity-level CLI commands, if a shared deployment is approved.
