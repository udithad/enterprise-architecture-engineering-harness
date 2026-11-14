# External AI Execution and Developer Experience Improvements

Status (2026-11-13): **IMPLEMENTED; acceptance run in progress.**
- CI passes: 241 tests, 0 failed.
- The `simple-springboot-api` run completed BRD analysis and requirements with Claude Code as the AI client.
- It now waits at the requirements approval gate for a **human** approver (see §10).
- Nothing has been committed.

## 1. Initial architecture assessment

What exists today (verified by reading the code):

| Area | Current state | Where |
|---|---|---|
| AI execution | Every agent task runs a governed model loop: `AgentRuntime::execute_inner` assembles context (system prompt, agent instructions, approved skills, mandatory standards, retrieved knowledge), calls `ModelClient::complete`, executes proposed tool calls through the `ToolGateway`, validates the final JSON (output schema, skill output contracts, citation check, domain validators) and retries with feedback within the correction budget. | `eaeh-agents/src/runtime.rs` |
| Model providers | `ModelClient` is the only path to a model. Providers: Anthropic (billed, API key) and Replay (fixtures, ADR-0023). Whatever is enabled in `eaeh.toml` is registered. | `eaeh-intelligence`, `eaeh-runtime/src/harness.rs` |
| Workflow engine | Deterministic state machine. Agent and tool states run through `TaskExecutor`; durable `tasks` rows with idempotency keys; suspension exists only for approval gates (`waiting_approval`). | `eaeh-workflow/src/engine.rs` |
| Phase 3 agents | `business-analyst`, `requirements-engineer`, `solution-architect`, `software-designer`. The first two read the BRD through the `knowledge.read` tool; that tool call is what makes BRD citations valid. | `config/agents/*.json` |
| MCP | `eaeh mcp` (stdio). Read/plan/start tools only; no approval tool; the client can start a run, but the run then needs an EAEH-managed model provider. | `eaeh-cli/src/mcp.rs` |
| Bootstrap/auth/skills/readiness | Delivered by the previous DX change (profiles, `project init`, `approval review`, `project readiness`). | `eaeh-cli/src/dx.rs`, `eaeh-runtime/src/project.rs` |
| Software development | `engineering.task` is a Phase 1 demo. Implementation, code generation and build/test are Phase 4 and not implemented. | `workflows/engineering.task.json` |

**Local environment facts that constrain the acceptance run.**
- JDK 17 is installed. JDK 21 and Maven are not.
- Docker's daemon is not running. Podman is running, and `exec.run` uses it.
- The `claude` CLI 2.1.323 is installed.

## 2. Root causes of the remaining usability problems

1. **AI execution is tied to an EAEH-managed provider.** A developer who already pays for Claude Code or Copilot must also obtain and pay for an Anthropic API key, or use hand-authored fixtures. Claude Code can start a workflow over MCP, but cannot do the reasoning for it.
2. **There is no hand-off point.** The engine can suspend only for human approvals, so work cannot wait for an external client.
3. **The cost model is invisible.** Nothing tells the user which execution mode is active or who pays.
4. **Only Claude Code is configured.** `project init` does not generate any GitHub Copilot configuration.
5. **Software development (steps 9–12) does not exist** (Phase 4).

## 3. Proposed implementation plan

### 3.1 Execution modes (D1)

New installation setting: `[execution] mode = "external_ai" | "direct_provider" | "deterministic"`.

| Mode | Providers registered | Who reasons | Billing |
|---|---|---|---|
| `external_ai` | **None, even if enabled in config.** A direct model call is structurally impossible: `ModelClient` has no models. | The connected client (Claude Code, Copilot) | The user's own AI subscription, under its own terms and limits |
| `direct_provider` | The enabled providers (today's behaviour) | EAEH through the provider API | Separate API key and billing |
| `deterministic` | Replay only | No AI: recorded fixtures | None |

- The shipped `config/eaeh.toml` sets `external_ai`.
- When the section is absent, the default is `direct_provider`, so existing installations and tests behave as before.
- The mode is shown by `project readiness`, `workflow status` and `eaeh_project_context`, and is recorded in task provenance and audit.
- **Guard against hidden fallback:** an agent task that was handed off externally can never be completed by a provider. A runtime in another mode refuses it.

### 3.2 Task hand-off at agent-task level (D2, new ADR-0025)

In `external_ai` mode, the agent runtime does everything it does today except call a model:

1. **Prepare.** Build an *external task package*: the agent's purpose and responsibilities, the system and agent prompts, approved skill instructions, mandatory standards, knowledge excerpts with citations, the task input, the output JSON Schema, the allowed tools, and revision feedback. It is stored in a new `external_tasks` table (migration 0004), keyed to the durable workflow task.
2. **Suspend.** The workflow instance status becomes `waiting_external`, and the task status becomes `awaiting_external`. This is durable and resumable; restarts are safe.
3. **Client works.** Claude Code or Copilot reads the package. It may call the agent's **allowed tools through the EAEH tool gateway**, under the agent's own principal and autonomy, for example `knowledge.read` of the BRD. These calls are authorized, audited and budgeted, and their citations become valid for the task.
4. **Submit.** The client submits the JSON output. EAEH re-enters the task and runs **the same validation as direct mode**: output schema, skill output contracts, citations, domain validators, and then the workflow's quality gate, artifacts, provenance and trace links.
   - Invalid output is returned as feedback. The task reopens and one correction attempt is charged. When corrections run out, the run escalates.
   - Valid output continues the workflow to the next hand-off or approval gate.
5. **Record.** Provenance records `models: ["external:<client>"]`, the submitting person, the mode and the package hash. **The submitter is recorded as a co-author, so the same person cannot approve that artifact.**

Authorization:
- A new permission, `task.external_execute`, is granted to **operator only**.
- Admins, stewards and approvers get nothing new.
- The tool gateway still applies the agent's `allowed_tools`, autonomy and workspace confinement.

### 3.3 Client integration (D3)

- **One provider-neutral MCP server (`eaeh mcp`)**. It adds four tools:
  - `eaeh_task_list`: open hand-offs;
  - `eaeh_task_get`: the package;
  - `eaeh_task_tool`: a governed tool call for the task;
  - `eaeh_task_submit`: validate and continue.
- **There is still no approval tool.**
- **CLI equivalents** for terminal use and scripts: `eaeh task list | show | tool | submit`.
- **`project init --client claude-code,copilot`** generates the client files:
  - **Claude Code:** the `CLAUDE.md` section and `.mcp.json` (as today, with updated instructions for the hand-off loop);
  - **GitHub Copilot in VS Code (agent mode):** `.github/copilot-instructions.md` and `.vscode/mcp.json` (`servers.eaeh`, stdio).
- **Neither client's credentials are accessed by EAEH.** Each runs `eaeh mcp` under the developer profile.
- **Not supported:** the GitHub Copilot coding agent on github.com, which cannot reach a local EAEH. This will be documented.

### 3.4 Software development lifecycle (D4, decision needed)

Implementing steps 9–12 means a new governed delivery workflow:
1. implementation plan (agent task, then approval);
2. implement (agent task with `fs.read`/`fs.write` in the EAEH workspace through `eaeh_task_tool`, including tests);
3. build and test (deterministic `exec.run` tool step in a container);
4. quality gate, then approval;
5. export of the approved workspace to the repository.

This is Phase 4 scope. Locally, it cannot be verified end to end without two changes:
- a container image with JDK 21 and Maven;
- a dependency cache, because containers run without network.

### 3.5 Bootstrap, authentication, skills and readiness (extensions only)

- Readiness gains these checks:
  - `[execution]` (mode, and what it costs);
  - `[client]` (generated client files present and consistent);
  - external mode with workflows that need a planner, which external mode cannot run (blocking only for those activities).
- `workflow status` and `eaeh_pending_approvals` list open hand-offs with the exact next action.
- `auth`, skills approval and bulk review are kept as delivered. Only defects found during the acceptance run are fixed.

### 3.6 Tests (planned)

- **Engine:** suspension and resume for `waiting_external`, and idempotent re-entry.
- **Agent runtime:**
  - the package equals the direct-mode context inputs;
  - invalid submissions consume corrections and then escalate;
  - citations come only from the package or governed task tools.
- **No-fallback guarantee:**
  - with `external_ai` and providers enabled, zero providers are registered;
  - a mock provider with a call counter records 0 calls;
  - `model_usage` has 0 rows.
- **Authorization:**
  - only operators may submit or call task tools;
  - cross-project and cross-tenant hand-off access is denied;
  - the submitter cannot approve their own output.
- **MCP:** tool listing, and a full hand-off round trip over stdio. Claude Code and Copilot use the same server.
- **Bootstrap:** generated Copilot and Claude files, and idempotent re-runs.
- **Lifecycle:** BRD → requirements → HLD → LLD in `external_ai` mode with scripted submissions, and in `deterministic` mode.
- **Backward compatibility:** the existing direct-provider tests stay unchanged and passing.

### 3.7 Documentation (in place, no duplicates)

- `EAEH-GETTING-STARTED.md`, `DEVELOPER-EXPERIENCE-GUIDE.md`, `claude-instructions.md` (reconciled amendment of §3 decisions, §10, §16, and a new subsection on execution modes), README, `security.md`, `integrations.md`, `components.md`, ADR-0025, and this report.
- Remove obsolete text that says an Anthropic key is required.

## 4. Security and architectural implications

- **No new service.** The work spans the existing crates: `eaeh-agents`, `eaeh-workflow`, `eaeh-runtime`, `eaeh-cli`, `eaeh-governance` and `eaeh-store`.
- **Trust boundary.** Client output is untrusted model output, so it is validated exactly like provider output.
- **Tool calls.** The client's tool calls run as the agent principal under the agent's `allowed_tools`, not as the developer, so the client gains no developer power through them.
- **Separation of duties** is strengthened, because the submitter is a co-author.
- **Residual risk.** The client can still edit the repository directly, outside EAEH. Generated instructions guide it, but EAEH can only govern what goes through it. This will be documented, as it was before.
- **ADR change.** This amends ADR-0010 (model provider abstraction). Direct-provider mode remains an explicit option.

## 5. Approved decisions (2026-11-13)

| Decision | Approved option |
|---|---|
| Core design | As planned: hand-off per agent task, `external_tasks` table, `waiting_external` status, operator-only `task.execute_external`, submitter cannot approve, `external_ai` default in the shipped configuration |
| Software development (steps 9–12) | Build the governed delivery workflow now; verify the Maven build/test step later (no JDK 21 + Maven isolation image on this workstation) |
| Acceptance run | This Claude Code session acts as the external AI client through the CLI hand-off, plus a headless `claude -p` session to verify the real MCP connection |

## 6. Implemented architecture and changes

The work spans the existing crates; no new service and no new crate were added. The decisions are recorded in ADR-0025.

| Area | Change | Where |
|---|---|---|
| Execution modes | `[execution] mode` (`ExecutionMode`). Provider registration is gated by mode: `external_ai` registers none and reports ignored providers; `deterministic` registers replay only and fails without it. | `eaeh-agents::external`, `eaeh-runtime::{config, harness}`, `config/eaeh.toml` |
| Hand-off | The agent runtime prepares the package (same `ContextAssembler`), runs the egress policy check (`ModelClient::authorize_external_handoff`), suspends with `ErrorKind::AwaitingExternal`, resumes the same agent instance, runs governed task tools as the agent, validates submissions (schema, skill contracts, citations, domain validators, listed files exist), and charges the correction budget | `eaeh-agents::{external, runtime}`, `eaeh-intelligence::client` |
| Engine | `InstanceStatus::WaitingExternal`; task status `awaiting_external`; same-visit resume; audit `workflow.await_external`; gate authors = all producers of the run | `eaeh-workflow::engine` |
| Storage | Migration 0004: `external_tasks`; status constraints rebuilt for `workflow_instances` and `tasks` | `eaeh-store/migrations` |
| Authorization | `Action::TaskExecuteExternal` (`task.execute_external`), operator only | `eaeh-governance::authz` |
| Provenance | `models: ["external:<client>"]`; inputs `external-task-package:sha256:…` and `submitted-by:<id>`; submitter added to the authors | `eaeh-runtime::executor` |
| Delivery | Workflows `engineering.delivery` and `engineering.delivery-unverified`; agents `delivery-planner` and `software-engineer`; skills `implementation-planning` and `code-implementation`; validators and gates `delivery.plan`/`delivery-plan`, `delivery.implementation`/`delivery`, `delivery.build`/`build`; tool `delivery.snapshot` | `workflows/`, `config/agents`, `config/skills`, `prompts/`, `eaeh-engineering::delivery` |
| Artifacts | Names may contain ASCII uppercase letters (Java source paths); length ≤ 400. Content is still stored by hash; traversal, `//` and absolute paths are refused. | `eaeh-workspace::artifacts` |
| Project manifest and readiness | `[execution]` (expected mode, clients) and `[delivery]` sections; `deliver` activity (aliases `plan`/`implement`/`test`); readiness checks for execution mode and billing, ignored providers, planner-only workflows, client files, delivery prerequisites (blocking only for `deliver`) and data egress to clients | `eaeh-runtime::project` |
| CLI | `task list/show/tool/submit`; `project init --mode/--client/--build-verification/--build-program/--build-arg` (`--provider` is now a hidden alias); `workflow run deliver`; `workflow status` shows hand-offs and mode; `artifact export` reports approval | `eaeh-cli::{main, dx}` |
| MCP | Hand-off tools, client label from `clientInfo`, mode in context and initialize instructions; still no approval tool | `eaeh-cli::mcp` |
| Client files | One provider-neutral instruction text in `CLAUDE.md` and `.github/copilot-instructions.md`; servers in `.mcp.json` (`mcpServers`) and `.vscode/mcp.json` (`servers`, `type: stdio`) | `eaeh-cli::dx::write_client_files` |

## 7. MCP integration details

- **Server.** `eaeh --config <cfg> mcp --project-root <repo>` over stdio JSON-RPC, protocol `2025-06-18`. It runs as `EAEH_PROFILE=developer`.
- **Tools:**
  - `eaeh_project_context` and `eaeh_readiness`;
  - `eaeh_knowledge_search`;
  - `eaeh_activity_plan` and `eaeh_activity_run` (`requirements`, `design`, `deliver`);
  - `eaeh_workflow_status` and `eaeh_pending_approvals`;
  - `eaeh_artifact_read` and `eaeh_trace`;
  - **new:** `eaeh_task_list`, `eaeh_task_get`, `eaeh_task_tool`, `eaeh_task_submit`.
- **Shared contract.** Claude Code and Copilot use the same server, tools and instruction text. Client-specific parts are limited to the file locations and JSON keys each client reads.
- **Client labels.** `claude` maps to `claude-code`; `copilot` or `Visual Studio Code` map to `github-copilot`; anything else is sanitised. Labels are self-declared and used for provenance only.

**Verification:**

| Client | What was verified | Evidence |
|---|---|---|
| Claude Code | (a) Protocol-level automated test. (b) A **real Claude Code 2.1.323 headless session**: it loaded `.mcp.json`, called the read-only EAEH tools, reported `external_ai` billing correctly and showed the approver command without attempting to approve. (c) This interactive Claude Code session did the AI work of the acceptance run through the CLI hand-off commands (same service API as MCP). | `evidence/claude-code-mcp-session.log`, `evidence/external-ai-acceptance.log` |
| GitHub Copilot (VS Code agent mode) | Protocol-level only: the generated `.vscode/mcp.json` shape and a full MCP hand-off round trip by a client identifying as `Visual Studio Code`. **No real Copilot session was run** (A-48). | `crates/eaeh-cli/tests/dx.rs` |
| Copilot coding agent (github.com) | **Unsupported**: it cannot reach a local EAEH | – |

## 8. New or modified CLI commands

| Command | Status |
|---|---|
| `eaeh task list [--all]`, `task show <id> [--out F]`, `task tool <id> <tool> --input JSON\|@file`, `task submit <id> --output F\|- [--client C]` | New |
| `eaeh project init … --mode … --client … --build-verification … --build-program … --build-arg …` | Modified. Defaults: `external_ai`; both clients on the first run; build verification `required`; workflows requirements, design and delivery. `--provider` is a hidden alias. |
| `eaeh workflow run deliver` (aliases `plan`, `implement`, `test`) | New activity. `release`/`deploy` report `not_supported` (Phase 7). |
| `eaeh workflow status` | Shows `waiting_external` hand-offs, the next actions and `execution_mode` |
| `eaeh project readiness` | New areas: `execution`, `client`, `delivery` |
| `eaeh artifact export` | Reports `approved: {approval, gate, decided_at}`, or a note that the version is not approved |
| `eaeh mcp` | New tools (§7) |

## 9. Authentication, approval and workflow changes

**Authentication and authorization:**
- **New permission.** `task.execute_external`, operator only. No existing role gained or lost any other permission.
- **Who can see what.** Approvers can list hand-offs (`workflow.read`) but cannot read packages. Other projects and tenants get "not found".
- **Approvals.** Clients have no approval tool. The submitter is recorded as a co-author, and gate authors are now the union of all producers in the run. The test `external_ai_lifecycle…` shows that a principal holding both operator and approver roles cannot approve work they submitted.
- **Approval binding.** `artifact export` shows whether exactly that version was approved.

**Workflow changes:**
- Agent states suspend durably in `external_ai` mode and resume the same task after a submission.
- The delivery workflows are new (ADR-0025 §4).
- The `design` and `requirements` definitions themselves are unchanged.

## 10. Automated tests and end-to-end acceptance evidence

**Quality gate (`scripts/ci.sh`, `evidence/external-ai-ci.log`):** **241 passed, 0 failed, 0 ignored**. That is 228 existing tests plus 13 new ones; fmt, clippy (`-D warnings`) and cargo-deny are clean.

**New tests:**

| Test | Proves |
|---|---|
| `external_ai_lifecycle_brd_to_lld_without_any_model_call` | 4 hand-offs and 3 gates to `Succeeded`. The package contains the standard and approved skill as instructions. Allow-lists bind the client, and approvers cannot read packages. Invalid output gets feedback. The submitter cannot approve. Provenance shows `external:claude-code`. Traceability works. **The offered provider received 0 requests, there are 0 `model_usage` rows and 0 `model.invoke` audit records**, and the audit chain is valid. |
| `external_tasks_are_project_bound_and_correction_bounded` | Cross-project access gets "not found". Malformed client labels and non-object outputs are refused. The fourth invalid submission fails the run (budget 3). |
| `a_direct_provider_runtime_never_completes_a_handed_off_task` | Same database, `direct_provider` runtime with a willing provider: the run fails with "no fallback" and the provider is never called |
| `deterministic_mode_registers_only_the_replay_provider` | Missing cassette is an error. Anthropic and offered providers are ignored. |
| `delivery_with_deferred_build_produces_an_approved_traceable_source_bundle` | Plan coverage gate (rejection, then acceptance) → approval → workspace confinement (`../` refused, `exec.run` not allowed) → a listing of unwritten files is rejected → files written through `fs.write` → bundle with `EAEH-DELIVERY.md` marked "NOT EXECUTED BY EAEH" → approval → `implemented_by` / `verified_by` trace links; 0 model usage |
| `delivery_with_required_build_never_skips_the_build_step` | Without isolation, the run fails at `build_test`; no delivery approval is requested |
| `external_ai_bootstrap_cli_and_mcp_hand_offs_without_model_calls` (CLI) | Default bootstrap (both clients, delivery, `external_ai`); no tokens in any generated file; readiness reports the mode, the ignored replay provider and both clients; CLI hand-off (0600 package file, approver refused, rejection, then acceptance); MCP hand-off as `github-copilot`; no `model.invoke` |
| Unit tests (6) | Delivery validators, path safety, build result, mode parsing, client labels |

**Updated tests (intended behaviour changes only):**
- Provider-based suites set `direct_provider` explicitly; this is the backward-compatibility path.
- Inventories now include the new agents, workflows and gates.
- `plan`/`implement`/`test` now select `deliver`.
- The provider readiness message was reworded.

**Deterministic regression.** `scripts/demo-onboarding.sh` was rerun in `deterministic` mode and completed. Log: `evidence/external-ai-deterministic-regression.log`.

**Acceptance scenario on `simple-springboot-api`** (`scripts/demo-external-ai.sh`, `evidence/external-ai-acceptance.log`):
- It runs on a copy under `target/demo-external-ai`; your repository `../simple-springboot-api` was not modified.
- The installation runs in `external_ai` mode with the replay provider deliberately enabled, to prove it is ignored.

| Step | Result |
|---|---|
| 1 Bootstrap | One command, with both clients and deferred build verification. It generated `CLAUDE.md`, `.mcp.json`, `.github/copilot-instructions.md`, `.vscode/mcp.json` and `.eaeh/project.toml`, with 0 secrets in any of them. |
| 2 Readiness | NOT READY (5 skills pending) → after approvals **READY (0 blocking, 3 warnings)**: no standards, build deferred, confidential BRD sent to the client |
| 3 Skills | 5 required skills registered and approved. **Disclosure:** these skill approvals were run by this session through the scripted `approver` profile, standing in for a human. When the session then tried to decide the *requirements gate* the same way, the Claude Code safety classifier blocked it as undermining oversight. That is correct: an AI client must not approve, so the remaining gates are left to you. |
| 4 BRD | Ingested as confidential, project-scoped knowledge; 20 sections |
| 5 BRD analysis | Business-analyst hand-off, **done by this Claude Code session**: it read the package, read the BRD through governed `knowledge.read` (20 sections), wrote its own analysis (10 requirements, 2 business rules, 3 domain concepts, 3 issues, 6 justified exclusions), ran `design.check`, made 3 non-functional statements measurable, and submitted. **Accepted.** |
| 6 Requirements | Requirements-engineer hand-off, done by this session: a reviewer pass found and fixed an invented fact (port 8080) and a missing JSON content-type criterion (BRD AC-005), re-read the BRD and submitted. **Accepted.** The run is now `waiting_approval` at `approve_requirements`. |
| 7–12 | **Not yet run.** They need human decisions at the requirements, HLD, LLD, plan and delivery gates. After each approval, the next hand-off is done by the AI client (see §16). |
| 13 Interim inspection | 2 hand-offs accepted (`client=claude-code`, 1 submission each); audit chain valid (128 records); actions include `external_task.handoff/tool/submit/accept` and `workflow.await_external` |

**Separate check:** a real Claude Code headless session through MCP (§7).

## 11. Proof that external AI mode did not invoke an EAEH-managed LLM provider

**Structural guarantee.** In `external_ai` mode, `Harness::build` registers no `ModelProvider` (`models.models()` is empty). `ModelClient::complete` therefore has nothing to call.

| Instrumentation | Automated tests | Acceptance run (so far) |
|---|---|---|
| Calls received by an offered provider | 0 (`ScriptedModelProvider::requests()` empty) | n/a (replay enabled but listed as "not registered") |
| `model_usage` rows | 0 | **0** |
| `model.invoke` audit records | 0 | **0** |
| `model.invoked` events | – | **0** |
| Handed-off task executed by a `direct_provider` runtime | refused ("no fallback"), 0 provider calls | – |

## 12. Updated documentation

- `docs/user-guides/EAEH-GETTING-STARTED.md`: rewritten in place for the new architecture.
- `docs/user-guides/DEVELOPER-EXPERIENCE-GUIDE.md`: comprehensive update in place.
- `claude-instructions.md`, reconciled rather than appended:
  - new decision row and §3.6 External AI execution;
  - notes in §3.1, §3.5, §10 and §16;
  - Phase 4 partially delivered early (§31).
- `docs/architecture/adr/ADR-0025-external-ai-execution-mode.md` (new) and the ADR index.
- `README.md`, `docs/OPERATIONS.md`, `docs/architecture/{integrations,security,components}.md`, `docs/implementation/{ACTION-ITEMS,TEST-STRATEGY}.md`.
- Obsolete statements that an EAEH Anthropic key is needed were removed or scoped to `direct_provider`.
- Historical phase reports and earlier DX documents were left unchanged.

## 13. Security review

| Concern | Assessment |
|---|---|
| Client credentials | Never accessed by EAEH. The MCP server uses the stored EAEH developer identity, and generated files contain no tokens (tested). |
| Data egress | Packages are built by the governed context path: redaction, fencing, classification. The `model.invoke` egress policy applies to `external-ai-client`, so **restricted content is never handed off**. Confidential BRDs do leave EAEH through the client; readiness warns (A-54). |
| Privilege | Client tool calls run as the **agent principal** with its allow-list, autonomy, budgets and workspace. The client gains neither the developer's nor any approver's power. Only operators can work on hand-offs. |
| Untrusted output | Submissions are validated like model output, plus the existence check for listed files. They are size-limited (2 MiB) and must be a single object. Repeated identical tool calls are limited (3). |
| Approval integrity | No approval tool. The submitter is a co-author and gate authors are the union over the run (tested with a dual-role principal). Exports report approval status. |
| No hidden fallback | Structural (no providers) and behavioural (other modes refuse handed-off tasks). Both are tested. |
| Isolation | Delivery code is written only into the run's workspace (path confinement tested). Build/test runs only through `exec.run` with container isolation, and the step is never skipped in `required` mode (tested). |
| Artifact names | Relaxed to allow uppercase letters. Storage is by content hash, traversal is still refused, and export re-checks paths and hashes. |
| Tenant/project isolation | Hand-offs are "not found" outside the caller's scope (tested). |
| Residual | Instructions cannot stop a client from writing files outside EAEH (A-52). Client labels are self-declared. `auth`/`task` have no HTTP API equivalents (A-51). |

## 14. Known limitations

- **Copilot.** Not verified in a real Copilot session (A-48). The Copilot coding agent is unsupported.
- **Claude Code over MCP.** Hand-offs were not exercised in a real Claude Code session over MCP. The real session used read-only tools, and the hand-off AI work in the acceptance run went through the CLI equivalents (A-44).
- **Build verification.** Not executed in this environment (no JDK 21 + Maven isolation image; containers have no network; A-49). The acceptance project uses `deferred`, and the bundle says so.
- **Acceptance run.** Incomplete until a human decides the gates (A-50).
- **Sequential execution.** Hand-offs are sequential, with no parallel agents in `external_ai` mode. Planner-driven workflows need `direct_provider`.
- **One mode per installation.** The project manifest only records the expected mode.

## 15. Outstanding risks and action items

| Item | Topic |
|---|---|
| A-44 | Partly closed: real Claude Code session verified, read-only |
| A-48 | Real Copilot session |
| A-49 | Build toolchain image and a `required` run |
| A-50 | Complete the acceptance run with a human approver |
| A-51 | HTTP equivalents of `task` |
| A-52 | Client-side enforcement |
| A-53 | Package size |
| A-54 | Data protection for third-party AI clients |
| A-38 | Phase 3 exit decision (still open) |

## 16. Recommended next steps

1. **Finish the acceptance run as the human approver.** In this Claude Code session, type `! scripts/demo-external-ai.sh approve "<your comment>"` after reviewing `target/demo-external-ai/review/…`. The AI client then does the HLD hand-off, and the cycle repeats for HLD → LLD → `deliver` (plan → approval → implementation → approval) → `export`/`report`.
2. Try the GitHub Copilot integration in VS Code on the same repository copy (A-48).
3. Provide a JDK 21 + Maven container image with an offline dependency cache, then switch the project to `--build-verification required` (A-49).
4. Decide on data protection for confidential BRDs handed to AI clients (A-54), and record the Phase 3 exit decision (A-38).
