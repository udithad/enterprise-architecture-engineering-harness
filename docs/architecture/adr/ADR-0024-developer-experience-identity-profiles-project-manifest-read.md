# ADR-0024: Developer experience: identity profiles, project manifest, readiness, activities and MCP integration

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-13 |
| Phase | Developer-experience improvement (within the Phase 1–3 scope) |
| Deciders | Product owner (developer-experience request), Principal architect |

## Context
Onboarding a real project (`simple-springboot-api`) required:
* a single `EAEH_TOKEN` switched between administrator, steward, approver and operator identities;
* 15–25 manual commands with copied UUIDs, digests and hashes;
* knowledge of internal workflow inputs (`{"brd": "source/subject@vN"}`);
* no way to see missing prerequisites before a failing run;
* error messages without remedies;
* no integration with Claude Code.

The separation of roles itself is intentional (ADR-0011, ADR-0017) and must not be weakened.

## Decision
* **Identity profiles** (CLI): a local credential store in `$EAEH_HOME` (directory 0700, file 0600, atomic writes; refused if group/world readable). It is managed with `eaeh auth login | status | use | renew | logout`. Tokens are never printed, and `token issue/rotate` require a destination (file, profile, or explicit `--print`).
* **Profile selection order:**
  1. `--as` / `EAEH_PROFILE`;
  2. `EAEH_TOKEN`;
  3. automatic, only when exactly one stored identity is authorized for the command's permission;
  4. the default profile.

  **Approval decisions are never auto-selected** (explicit `--as` or an approver default), so one person holding several local profiles cannot silently act as approver.
* **Diagnostics:** expiry, revocation and missing credentials are diagnosed from local metadata, with no new server-side oracle. Forbidden errors name the permission and the roles that grant it. Every CLI error carries a `hint`.
* **Project manifest** `.eaeh/project.toml` (no secrets) records project id, workspace and git association, stack, constraints, BRD citation, workflows, role holders and provider mode. Commands find it from the working directory.
* **`eaeh project init`** (administrator, idempotent) composes existing governed operations:
  * project creation;
  * principals created with exactly one role, or existing ones granted project access via the new `IdentityService::grant_project_access` (`principal.manage`, audited, roles never changed);
  * tokens as stored profiles or hand-over files;
  * project profile, knowledge, project standards (domain scope, mandatory, need approval) and BRD;
  * required-skill discovery and registration from `definitions.skills_dir`;
  * `CLAUDE.md` and `.mcp.json`;
  * readiness.
* **Readiness** (`eaeh project readiness`, exit 7 if not ready) checks project/workspace, identity and access, responsibilities (developer ≠ approver), workflows, gates, agents, tools, skills, BRD visibility, retrieval, standards, provider and audit chain. Each finding carries an exact fix.
* **Skills and approvals:**
  * `eaeh skill sync | pending`.
  * `eaeh approval review` shows each skill or standard in full and requires a per-item confirmation, or `--yes` after the listing. Every decision goes through `ApprovalManager` (human, role, SoD, digest/hash).
  * `KnowledgeService::document_for_approval` lets approvers see pending documents (authorized by `approval.read`) without granting `knowledge.manage`.
* **Activities:**
  * `eaeh workflow run requirements|design` resolves project, BRD and workflow, checks prerequisites, explains agents, skills and approvals, and starts only with `--yes`.
  * `workflow status` and `workflow resume` report failure stages and recovery.
  * Phase 4 activities are refused explicitly.
* **Claude Code:** `eaeh mcp` is an MCP stdio server running as the developer profile. Its tools cover context, readiness, knowledge search, activity plan/run, status, pending approvals (read-only, with the approver command), artifacts and trace. It has **no approval tool**. The generated `CLAUDE.md` section instructs Claude Code to route governed activities through these tools.

## Consequences
Onboarding takes three commands per role holder instead of dozens. Governance is unchanged: nothing was relaxed and every operation passes the same authorization, policy, approval and audit paths. Limitations:
* Claude Code compliance with `CLAUDE.md` is instruction-level; EAEH can only enforce what passes through it.
* Tokens are stored in a plaintext 0600 file (no OS keychain yet).
* CLI and MCP processes write to the same SQLite database (A-20).

## Alternatives considered
Interactive wizard (rejected for now: flags and an idempotent re-run are scriptable and testable); a VS Code extension (rejected: MCP plus generated instructions suffice); automatic approval of repository skills (rejected: violates human approval).
