# EAEH Automated Setup – Implementation Plan (DRAFT, awaiting approval)

Scope: Phases 1–3 only. No roadmap change. No commits. Nothing below is implemented yet.

## 1. Findings from inspection (read-only)

| Area | What exists | Consequence for `eaeh setup` |
|---|---|---|
| `eaeh init` | Creates data dir, applies migrations (`Harness::builder().build()`). | Reuse for DB init. |
| `eaeh admin bootstrap` | `IdentityService::bootstrap` – creates tenant + project + admin **only when zero principals exist**; token shown once or written to file. | Reuse for first run. If principals exist but no usable admin credential there is **no** existing recovery path. |
| `eaeh project init` | `dx::register_project` (needs an admin identity and a project folder): project, developer/steward/approver principals, role checks, token issue, profile storage, skill sync, readiness. | Reuse `ensure_principal`, `store_issued`, token logic. Setup does **not** need a project folder; it works at tenant/default-project level. |
| Credential store | `credentials.rs`: profiles bound to `data_dir`, 0600 file, 0700 dir, atomic write. `Profile.principal/roles/token_id/expires_at`. | Reuse. Add a verification step that authenticates the token and compares the resolved principal + roles with the intended ones (the existing `has_profile` check only compares the principal in one place). |
| Skills | `eaeh_runtime::project::discover_skills` (registers new/changed skill dirs as pending approval; content-hashed; duplicate-safe), `sync_skills`, `skill_state`. Format support (SKILL.md frontmatter, TOML/JSON) already exists (COMMUNITY-SKILL-FORMAT). | Reuse; setup only orchestrates and reports counts (discovered / registered / already / changed / pending). No auto-approval. |
| Portal | `portal::serve` – foreground, loopback only, per-run launch secret, `--approver/--steward/--operator/--admin` profiles, port-in-use error. | Reuse. Needs a small seam to (a) probe if a portal is already up and (b) run detached. |
| Health / readiness | `Harness::health`, `proj::readiness`, `mcp` config, worker status (`worker.rs`). | Reuse for checks. |
| Backup | None. SQLite WAL database `<data_dir>/eaeh.db`. | New: consistent backup via `VACUUM INTO` (WAL-safe) + copy of `credentials.json`, into `0700` dir. |

## 2. Design

New module `crates/eaeh-cli/src/setup.rs` (CLI orchestration only; no new identity/approval service) plus small, tested helpers:

* **Check engine** – ordered list of `Check { id, status: ok|repaired|warn|blocked, detail, remediation }`; every issue carries root cause, `safe_to_repair` flag, repair attempt (at most **one** per issue, then re-verify; no retry loops), and result. Results go to the setup report and, for identity/config mutations, to the audit log (via the existing audit writer used by the services; identity service already audits principal/token changes).
* **Identity matrix** (default local-dev): `admin→admin/admin`, `developer→dev/operator`, `approver→approver/approver`, `steward→steward/knowledge_steward`. Per profile: principal exists & enabled → exact role set equals the expected one (never admin on non-admin; extra privileged roles on a non-admin = **blocked**, never silently changed) → project access → token authenticates **as that principal** → not expired/revoked → `permits()` matches expected actions. Mismatched profile (e.g. `developer`→admin) is **not overwritten**: reported as blocked with the exact `eaeh auth logout`/`--repair` instruction; `--repair` may replace only a profile whose token is expired/revoked/missing, or whose mapping is wrong **after** the previous credential is backed up to the timestamped backup dir’s metadata (principal/token-id only, never the token).
* **Admin unavailable** → fail safely: nothing mutated, exact recovery text (`eaeh auth login --profile admin --token-file …` if you have the token; otherwise `eaeh setup --reset`).
* **Phases of `eaeh setup`** (1–14 of the spec) run in order; any blocked mandatory check stops before Portal start.
* **Flags**: `--check` (read-only diagnose, exit 7 if not ready, like readiness), `--repair` (apply safe repairs, no prompts), `--reset` (backup + confirm + reinitialise), `--open`, `--yes` (non-interactive confirmation for reset, still requires typing the phrase via `--confirm-reset "RESET EAEH"`), `--bind`, `--no-portal`, `--tenant`, `--project`, `--ttl-days`. Interactive run on an existing install offers **A) repair/continue** or **B) reset**.
* **Reset** – warn (projects, workflows, approvals, audit, knowledge lost); require typed confirmation; create `<data_dir>/../eaeh-backups/<UTC-timestamp>/` (0700) containing `eaeh.db` (VACUUM INTO), `credentials.json` copy (0600) and `eaeh.toml`; only then remove `eaeh.db*`, re-init, bootstrap, re-issue profiles, re-register skills. Never touches skill dirs, source workspaces or `workspaces/`/`artifacts/` folders. Profiles of the old DB are replaced (they are dead anyway) after being backed up.
* **Portal start** – only after all mandatory checks pass; probe `portal state file` + TCP/HTTP to detect an already-running instance (reuse it); detect port conflict by foreign process and report; start with `--approver approver --steward steward --operator developer --admin admin`.
* **Secrets** – tokens never printed/logged; setup report is redacted; portal launch URL (contains a one-time secret) printed only on stdout and written to a 0600 state file.
* **Docs** – update `EAEH-USER-GUIDE.md` (replace manual bootstrap steps), add `docs/implementation/EAEH-SETUP-AUTOMATION.md`, ADR-0032 only if a decision deviates.

## 3. Tests (real, isolated – temp `EAEH_HOME`, temp data dir, ephemeral ports; never the dev DB)

CLI integration tests in `crates/eaeh-cli/tests/setup.rs` driving the real `eaeh` binary against a real SQLite DB, covering all 18 scenarios in the request (first-time, repeat/idempotent, healthy, missing principal, wrong mapping, expired/revoked token, wrong role, missing project access, missing admin credential, reset with/without confirmation, backup + restore, skill discovery/changed/pending, portal start / already running / port conflict, failed init + safe recovery). Portal tests start the real portal on `127.0.0.1:0`-style free ports and issue real HTTP requests. Expiry/revocation use real `token revoke` and a short-TTL/backdated token created through the service. Plus a final end-to-end run script using an isolated environment.

## 4. Decisions needed (see questions)

1. Portal process model after setup.
2. Default tenant / project names for first-time bootstrap.
3. What `--reset` removes.

## 5. Known limits to be documented (not hidden)

* Portal “running after setup” is a local dev process (no supervisor/service manager).
* External-AI readiness can verify configuration and that the `claude` CLI is on `PATH`; it cannot verify a signed-in subscription without making a model call (forbidden by ADR-0025) → reported as *configured, not verified*.
* If the admin token is lost and principals exist, the only recovery is reset (by design: no authorization bypass).
