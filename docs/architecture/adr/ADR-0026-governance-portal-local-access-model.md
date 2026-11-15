# ADR-0026: Governance Portal and its local access model

| Field | Value |
|---|---|
| Status | Accepted (plan `docs/implementation/stages/GOVERNANCE-PORTAL-PLAN.md`, decisions D1–D4 approved 2026-11-14) |
| Date | 2026-11-14 |
| Phase | Supplementary UI enhancement within the Phase 1–3 scope (`Approval-Portal.md`) |
| Deciders | Product owner (UI enhancement request), Principal architect |

## Context
Reviewers need a web UI that shows workflow progress and lets them read and decide Requirements, HLD and LLD documents. The request rules out authentication for the first version: no login page, OAuth/OIDC or user management. It also requires that backend authorization and approval enforcement stay unchanged.

The existing `/v1` API authenticates every request with a bearer token. In `external_ai` mode (ADR-0025), the developer's AI client runs on the same machine and must have **no approval capability**.

A local web server that answers without authentication and holds a stored approver token would give that capability to any local process. This ADR decides how the portal works without a login while keeping that property.

## Decision
1. **One process, no new service.** `eaeh portal` runs the existing harness in-process, on the same SQLite database (ADR-0004). It serves the built React UI (`portal/dist`) and mounts the **unchanged** `/v1` router under `/api`. Portal requests therefore pass the same authentication, authorization, rate limiting, approval manager, hooks and audit as any API client. The workflow engine resumes after decisions exactly as it does for the CLI (`Harness::decide_approval`).
2. **Identity injection instead of login (D1).** The portal acts as identities the developer already stored with `eaeh auth login` (ADR-0024):
   * reads use the viewer: `--as`, `EAEH_TOKEN` or the default profile;
   * `POST /v1/approvals/{id}/decision` uses only the profile named with `--approver`. That profile must be a human with the approver role. Approval identities are never auto-selected. Without `--approver` the portal is read-only.

   The portal injects the bearer token server-side and replaces any client-supplied `Authorization` header. Tokens never reach the browser.
3. **Local-only guard (D2).**
   * The portal binds loopback addresses only and refuses any other bind address.
   * `Host` must be the portal's loopback address (protection against DNS rebinding).
   * Non-GET requests must carry an allowed `Origin` (CSRF protection).
   * Every `/api` call needs a session cookie. The cookie value is a 256-bit per-launch secret. It is printed once to stdout in the launch URL, exchanged by the UI via `POST /portal/session` for an `HttpOnly; SameSite=Strict` cookie, and removed from the address bar. It is never written to disk.
   * Responses carry CSP (`default-src 'self'`), `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `nosniff`, and `no-store` on API responses.
4. **Allowlist.** Through the portal, only read endpoints and the decision endpoint are reachable. Token, principal, plugin, knowledge, skill, project and workflow start/cancel operations stay with the CLI and the authenticated API. In this version, decisions through the portal are accepted only for **workflow gates**. Skills and standards are reviewed with `eaeh approval review`, which shows their full content.
5. **Read models, no new write paths.** New read-only endpoints compose existing services, project by project, with the existing permissions (`workflow.read`, `approval.read`, `artifact.read`):
   * `GET /v1/dashboard`, `/v1/activity`, `/v1/approval-queue`, `/v1/workflow-queue`;
   * `/v1/workflows/{id}/detail`, `/v1/approvals/{id}/document`, `/v1/projects/{id}/overview`;
   * `GET /v1/audit?order=desc`.

   Projects the caller cannot read are reported as restricted, not as errors. Unknown and foreign resources both return `not_found`. New engine read methods: `tasks_for`, `active_tasks`, `list_active`, `status_counts`, `recent_activity`. There is no schema migration.
6. **Static UI from disk (D3).** `cargo build` never needs Node. `--static-dir` (or `EAEH_PORTAL_DIR`) points to the build output. In development, the Vite dev server proxies `/api` and `/portal` to `eaeh portal --dev-origin http://localhost:5173`.

## Consequences
* Reviewers read the full bundle (every file is hash-checked against the manifest), its metadata, quality-gate results, traceability and earlier requests. They then approve, request changes or reject, each with comments and confirmation.
* The UI re-reads the request before submitting and refuses to submit when the status or hash changed. The backend's hash check remains authoritative.
* The backend decides. A decision the approval service refuses (wrong role, author, stale hash, already decided) is shown with the service's message; the portal never writes workflow records.
* **Residual risks:**
  * A local process that can read the portal's stdout, or the user's browser cookie store, can act with the launch secret. Start the portal from your own terminal, not from an AI client's tool.
  * The secret lives for the life of the process.
  * Anyone who can use the browser profile on the same OS account can use an open session.
* **Not suitable beyond trusted local development.** Real authentication (enterprise identity, per-user sessions) must be introduced before the portal is exposed to other machines, users or tenants.
* Polling (10 s dashboard, 5 s active workflow, 15 s lists, paused in background tabs) instead of server push. This is acceptable for local SQLite.

## Addendum (2026-11-14): completion guard
After acceptance review, the workflow engine records `succeeded` only if every approval gate that leads to the success end holds a valid approval:
* human, not an author;
* `approve` decision;
* bound to the hash of the current artifact version.

Otherwise it ends `failed` (`completion guard: …`). Every check is audited as `workflow.completion_check`. This strengthens §3.5 enforcement at the workflow boundary and changes no approved decision. The portal shows the same per-gate verification (`WorkflowEngine::gate_checks`) and each artifact's generation source (AI client, recorded fixture, model provider, platform). See EAEH-GOVERNANCE-PORTAL.md §7a.

## Alternatives considered
* **SPA calling `/v1` with a pasted token.** Rejected: it is a login in disguise and exposes tokens to browser JavaScript.
* **Loopback and Origin checks only, without a secret.** Rejected: any local process, including the AI client, could approve through the stored approver identity.
* **Per-decision profile picker in the UI.** Rejected: it auto-surfaces approver identities; the launch flag keeps the choice explicit (ADR-0024).
* **Embedding the UI in the binary (`rust-embed`).** Rejected: new dependency, and `cargo build` would need Node.
* **Separate portal service or database.** Rejected by the request and by ADR-0002 (modular monolith).
