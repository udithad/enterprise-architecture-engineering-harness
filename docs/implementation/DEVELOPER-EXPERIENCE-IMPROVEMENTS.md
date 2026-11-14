# Developer Experience Improvements

> Status: **IMPLEMENTED and verified** (2026-11-13) within the Phase 1–3 scope. Sections 1–4 are the plan written before implementation; sections 5–11 report what was built and measured. Nothing has been committed.

## 1. Root causes of the onboarding friction

These were identified from the code paths a developer must follow (`simple-springboot-api`: BRD + bootstrap notes).

| # | Symptom | Root cause in the implementation |
|---|---|---|
| RC-1 | Constant token switching; "admin token cannot start the workflow" | One `EAEH_TOKEN` env var for every command. Roles are deliberately separated (the admin cannot operate or approve; the operator cannot ingest or approve), but the CLI has no notion of stored identities. |
| RC-2 | Long manual setup | Bootstrap creates only the admin. Principals, tokens, sources, BRD ingestion, skill registration and approvals are 15–25 separate commands, and UUIDs must be copied between them. |
| RC-3 | Skills unusable | Skill JSON lives under `examples/`, so each skill must be registered by hand. The approval id and digest must then be copied into `approval decide`. Nothing tells you which skills a workflow needs. |
| RC-4 | Workflow inputs unknown | You must know `{"brd":"source/subject@vN"}`. The citation format is internal. |
| RC-5 | Failures appear late | No readiness check. Missing provider, missing or unapproved skills, and a missing BRD surface only mid-workflow (e.g. `unavailable: no registered model …`). |
| RC-6 | Opaque errors | Errors print only `{code, message}`. The forbidden message names the permission but not the role that grants it. The `--roles` help omits `knowledge_steward`. |
| RC-7 | No project context | The project id must be passed on every command. Nothing links a repository to an EAEH project. Grant project access to an existing principal is impossible (only at creation). |
| RC-8 | No Claude Code integration | No MCP server and no generated instructions, so Claude Code does not know EAEH exists. |
| RC-9 | No offline execution | The only deterministic model is the dev-only test double (ADR-0010), so the shipped binary cannot run agent workflows without a paid provider. |

## 2. Plan (affected crates / modules)

| Item | Change | Crates |
|---|---|---|
| A. Identity store | `eaeh auth login/status/use/renew/logout`. Profiles are kept in a 0600 file under `$EAEH_HOME` (default `~/.config/eaeh`); tokens are never printed. Token selection: `--as <profile>` > `EAEH_TOKEN` > automatic (only for non-decision commands, and only when exactly one profile is authorized) > default profile. Diagnostics use stored metadata (expiry, roles). | `eaeh-cli` (new `credentials` module) |
| B. Project manifest | `.eaeh/project.toml` in the repository: project id, stack, constraints, BRD, workflows, role holders, provider mode. Commands walk up from the working directory, so `--project` becomes optional. | `eaeh-runtime::project` |
| C. One-command bootstrap | `eaeh project init` (admin, idempotent). It: <br>• creates or finds the project; <br>• associates the repository (path, git remote, HEAD); <br>• creates or grants the developer, steward and approver principals; <br>• issues their tokens as stored profiles (`--store-profiles`, single-person local setup) or as 0600 hand-over files; <br>• ingests the project profile, knowledge and BRD; <br>• discovers and registers the required skills; <br>• writes `CLAUDE.md` and `.mcp.json`; <br>• runs readiness. | `eaeh-cli`, `eaeh-runtime`, `eaeh-governance` (`grant_project_access`) |
| D. Readiness | `eaeh project readiness` (exit 7 if not ready). Checks project/workspace, identity and permissions, approver/steward responsibilities, skills, BRD and retrieval, standards, workflows and gates, provider, agents and tools, audit chain and egress. Each finding comes with an exact fix command. | `eaeh-runtime::readiness` |
| E. Skills | Shipped skill definitions move to `config/skills` (`definitions.skills_dir`). `eaeh skill sync`, `eaeh skill pending`, `eaeh skill approve [--all] [--yes]`: per-skill review and explicit confirmation; every decision goes through `ApprovalManager` (SoD unchanged). | `eaeh-cli`, `eaeh-runtime` |
| F. Workflow UX | `eaeh workflow run requirements\|design`: resolves project, BRD and workflow; runs prerequisite checks; explains agents, skills and approvals; requires `--yes`; executes; then prints the gate and next actions. `eaeh workflow status [id]` shows stages, failure stage and recovery. `eaeh workflow resume <id>`. | `eaeh-cli`, `eaeh-runtime::activity` |
| G. Errors | Every CLI error carries `hint`. A forbidden error names the roles that grant the permission. Hints cover unapproved skills, missing providers (with the deterministic option), workflow inputs (with an example) and failed stages. | `eaeh-cli`, `eaeh-governance` (message only) |
| H. Claude Code | `eaeh mcp`: an MCP server over stdio, running as the developer profile. Tools cover project context, readiness, knowledge search, workflow plan/run/status, pending approvals (read-only), artifacts, trace. **No approval tool.** Plus generated `CLAUDE.md` and `.mcp.json` (no secrets). | `eaeh-cli` (`mcp` module) |
| I. Deterministic mode | `[providers.replay]`: a production replay provider that returns recorded responses from a cassette file. It is explicitly enabled, its egress is `local`, it costs nothing, and its model version embeds the cassette hash, so provenance shows the output was not AI-generated. ADR-0023 (amends ADR-0010). | `eaeh-intelligence::replay`, `eaeh-runtime` |

## 3. Security and authorization implications

- **No new permissions.** Administrators gain nothing: bootstrap uses only `principal.manage`, `knowledge.manage` and `skill.manage`, which they already hold. Approvals still require a human approver who is not an author.
- **Explicit identity for approvals.** Automatic identity selection never applies to approval decisions. They need `--as <profile>` or a default profile that is an approver. This prevents silently acting as a second role.
- **Credential storage.** The store is a 0600 file in a 0700 directory, plaintext like `gh`/`kubectl` credentials. No OS keychain yet (limitation). Tokens are never printed or logged, and generated `.mcp.json`/`CLAUDE.md` contain no secrets.
- **MCP is no back door.** The MCP server is subject to the same authorization as the CLI. It exposes no decision tool, and governed activities run as the developer identity.
- **Replay provider.** Off by default. Readiness flags it with a warning. Provenance records `replay/…` with the cassette hash.

## 4. Out of scope (Phase 4)

Acceptance steps 10–12 belong to master §31 (Phase 4) and will **not** be implemented, because the brief forbids starting another phase:
- 10: implementation plan;
- 11: implementing the API with governed agents;
- 12: building and testing it.

`workflow run` reports these activities as unavailable.

## 5. What was implemented

| Plan item | Implemented as | Verified by |
|---|---|---|
| A. Identity store | `crates/eaeh-cli/src/credentials.rs`. `eaeh auth login\|status\|use\|renew\|logout`; global `--as` / `EAEH_PROFILE`. `token issue/rotate` need `--token-file`, `--store-profile` or `--print`. `IdentityService::authenticate_detailed` returns the token id and expiry. | `credentials::tests::selection_rules`, `dx.rs::identity_profiles_token_lifecycle_and_diagnostics` |
| B. Project manifest | `eaeh-runtime::project::ProjectManifest` (`.eaeh/project.toml`), `find_root`, `git_info` | `project.rs::manifest_roundtrip_and_discovery_from_subdirectories` |
| C. Bootstrap | `eaeh project init` (`crates/eaeh-cli/src/dx.rs`), `IdentityService::grant_project_access`, `eaeh principal grant` | `dx.rs::bootstrap_is_one_command_…`, `identity::tests::grant_project_access_…`, acceptance step 1/1b |
| D. Readiness | `eaeh-runtime::project::readiness`, `eaeh project readiness` (exit 7) | `project.rs::readiness_*` (3), `dx.rs::readiness_and_workflow_prerequisites_…` |
| E. Skills | Skills moved to `config/skills` (`definitions.skills_dir`). `required_skills`, `skill_state`, `sync_skills`; `eaeh skill sync\|pending`; `eaeh approval review`; `KnowledgeService::document_for_approval` | `project.rs::required_skills_…`, `dx.rs::approval_review_…` |
| F. Workflow UX | `plan_activity` and the `ACTIVITIES` table; `eaeh workflow run\|status\|resume` | `dx.rs::deterministic_onboarding_…`, `dx-extra-commands.log` |
| G. Errors | `hint_for` in the CLI: every error is `{code, message, hint}`. Forbidden messages list the granting roles (`RbacAuthorizer`). `Role::ALL` / `as_str`. `--roles` help includes `knowledge_steward`. | `dx.rs` (hints asserted), `project.rs` (role list) |
| H. Claude Code | `eaeh mcp` (`crates/eaeh-cli/src/mcp.rs`, MCP stdio, protocol `2025-06-18`), plus a generated `CLAUDE.md` section and `.mcp.json` | `dx.rs::mcp_server_exposes_governed_tools_without_approval_powers`, `dx.rs::bootstrap_…` |
| I. Deterministic mode | `eaeh-intelligence::replay::ReplayProvider`, `[providers.replay]`; hand-authored cassette `examples/simple-springboot-api/replay-cassette.json` | `replay::tests::…`, `dx.rs::deterministic_onboarding_…`, `dx-onboarding.log` |

Defects found and fixed while doing this:
- **Approvers could not see pending standards.** Listing pending documents required `knowledge.manage`. Fixed with an approval-scoped read (no role widened).
- **Planning hid a missing permission.** Planning as an identity without `workflow.start` reported "prerequisites satisfied". It now blocks with the fix.
- **Repeat bootstrap overwrote values.** A repeat `project init` replaced recorded values with empty flags. Omitted flags now keep the manifest values.
- **Replay log noise.** A replay-mode warning printed on every command. It is now an info-level log; readiness and provenance still disclose replay mode.

## 6. Architecture changes

- **No new crates or services.**
- **Changed crates:**
  - `eaeh-core`: `Role::ALL`, `Role::as_str`.
  - `eaeh-governance`: `grant_project_access`, `authenticate_detailed`, richer forbidden message.
  - `eaeh-intelligence`: `replay` module.
  - `eaeh-knowledge`: `document_for_approval`.
  - `eaeh-runtime`: `project` module, `[providers.replay]`, `definitions.skills_dir`.
  - `eaeh-cli`: `credentials`, `dx`, `mcp` modules.
- **Layering:** fitness layering is unchanged and passes. The CLI gained only the external dependencies `serde`, `chrono`, and the `io-std` feature of tokio.
- **ADRs:** ADR-0023 (replay provider; amends ADR-0010) and ADR-0024 (developer experience).

## 7. Tests executed and results

| Evidence | Result |
|---|---|
| `evidence/dx-ci.log` (2026-11-12T22:17:43Z UTC) – `scripts/ci.sh` | **228 passed, 0 failed, 0 ignored**: the 214 earlier tests plus 14 new. fmt, clippy `-D warnings` and `cargo deny` are clean; `==> CI PASSED`. |
| `evidence/dx-onboarding.log` – `scripts/demo-onboarding.sh` | Acceptance steps 1–9 and 13 completed; steps 10–12 reported `not_supported` (Phase 4). |
| `evidence/dx-extra-commands.log` | Exercised: `auth use`, `auth renew`, `workflow resume` (refused for a finished run), planning as approver (blocked), `principal grant` (idempotent). |

The 14 new tests:

| Area | Tests |
|---|---|
| Replay provider | 1 |
| Credential selection | 1 |
| Identity grant / authentication | 1 |
| Runtime project / readiness / skills / isolation | 5 |
| CLI | 6 |

Earlier tests changed:
- `crates/eaeh-cli/tests/cli.rs` now hands tokens over with `--token-file` instead of reading them from printed output. Its helper isolates `EAEH_HOME`.
- Skill fixture paths moved from `examples/skills` to `config/skills`.
- No test was removed or weakened.

## 8. End-to-end acceptance scenario (simple-springboot-api)

| # | Step | Result | How |
|---|---|---|---|
| 1 | Register/bootstrap | ✔ | `eaeh project init …` (one command); repeat run idempotent |
| 2 | Readiness | ✔ | `eaeh project readiness` – NOT READY (3 pending skills) with fixes |
| 3 | Knowledge and skills | ✔ | Registered by init: project profile, bootstrap notes, BRD, 3 skills |
| 4 | Human approvals | ✔ | `eaeh approval review --as approver --all --yes` (each item shown) |
| 5 | BRD identified | ✔ | Manifest `[brd]`; readiness READY (exit 0) |
| 6 | Traceable requirements | ✔ | `eaeh workflow run design --yes` – 8 requirements (3 functional, 5 non-functional), 2 business rules, 3 domain concepts, 1 ambiguity issue (BRD's "promptly"); BRD coverage 20/20 (14 covered, 6 excluded with reasons) |
| 7 | Approve requirements | ✔ | One `request_changes` round, then approve (hash-bound, `--as approver`) |
| 8 | HLD | ✔ | `hld.md`, 4 ADRs, 3 Mermaid diagrams; approved |
| 9 | LLD | ✔ | `lld.md`, ADR, 5 diagrams; approved; workflow `succeeded` |
| 10 | Implementation plan | ✘ not available | Phase 4 scope; refused with explanation |
| 11 | Implement with governed agents | ✘ not available | Phase 4 scope |
| 12 | Run tests and quality checks | ✘ not available | Phase 4/5 scope |
| 13 | Inspect artifacts, traces, approvals, audit | ✔ | Bundle export; `eaeh trace`; approval list; `eaeh audit list` (admin profile auto-selected); audit chain valid |

The model output in this scenario is **replayed, hand-authored fixtures**. It proves the governed pipeline and the developer experience, not AI generation quality. Model usage records `replay/replay` with version `replay-cassette-<hash>` and cost $0.

## 9. Backward compatibility and security review

| Change | Compatibility | Security |
|---|---|---|
| `EAEH_TOKEN` | Still works and takes precedence (unless `--as` is used) | Unchanged |
| `token issue/rotate` now require a destination | **Breaking for scripts that parsed the printed token**; `--print` restores the old behaviour | Fewer plaintext tokens in terminal scroll-back |
| `--project` optional on several commands | Explicit `--project` still works | Project resolved from the manifest is still authorized per call |
| `examples/skills` → `config/skills` | Paths updated in tests, scripts and docs. The Phase 2/3 completion reports keep their historical paths. | – |
| Forbidden message text | Text-only change: the error code is unchanged and the message now also lists the roles that grant the permission | Reveals only the static role model |
| Replay provider | Off by default | Local egress; labelled in provenance; readiness warning |
| MCP server | New, opt-in via `.mcp.json` | Same authorization; no approval tool |
| Credential file | New | 0600/0700, mode check, no printing; plaintext at rest (A-43) |

Not weakened:
- No role gained a permission.
- Approval gates, SoD, artifact-hash binding, quality gates and audit are unchanged.
- Administrators still cannot run workflows or approve.
- Agents have no new autonomy.

## 10. Outstanding limitations and risks

| Item | Risk / note | Action |
|---|---|---|
| No real Claude Code session was driven | MCP is verified at protocol level only | A-44 |
| `CLAUDE.md` is guidance | Claude Code can still edit files directly when told to | A-44 (hooks) |
| Plaintext 0600 credential file | Readable by the user's other processes | A-43 |
| CLI, MCP and server write one SQLite DB from several processes | `BEGIN IMMEDIATE` handles it, but this is not stress-tested | A-20 |
| Replay cassettes are tied to prompts and BRD citations | They become stale when either changes | A-46 |
| Acceptance steps 10–12 | Not available until Phase 4 | Phase 4 authorization |
| No live model run | Generation quality unverified (Phase 3 E-5) | A-30 |
| No HTTP API for the new commands | CLI and MCP only | A-45 |

## 11. Next actions

1. Run a real Claude Code session against `eaeh mcp` in the actual `simple-springboot-api` repository (A-44). Bootstrap it with `eaeh project init` there; this was intentionally not done to avoid modifying your repository.
2. Decide on OS-keychain storage (A-43) and the HTTP equivalents (A-45).
3. Perform the authorised live run (A-30) to replace replayed fixtures with real generation.
4. Record the Phase 3 exit decision (A-38) before authorizing Phase 4.
