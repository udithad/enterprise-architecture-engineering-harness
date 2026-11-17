# EAEH CLI-to-Portal parity inventory

| Field | Value |
|---|---|
| Status | Implemented 2026-11-17 (PORTAL-OPERATIONS-PLAN, W1–W4) |
| Source of the list | The `eaeh` command tree in `crates/eaeh-cli/src/main.rs` (24 top-level commands, 77 operations) |
| Enforced by | `tests/tests/parity.rs`: it parses the command tree and fails when an operation is missing here or listed twice |

**Classes:**
* **Action**: a Portal-supported action.
* **Read**: a Portal read-only capability.
* **CLI**: a CLI-only administrative or developer operation.
* **N/A**: not applicable to the Portal.

The portal never calls a CLI command. It calls the same `/v1` services, or, for local paths and credentials, the same service functions in the portal process. Every portal action runs as an explicit identity (`--approver`, `--steward`, `--operator`, `--admin`) and the backend authorizes it again. Anything not listed as Action or Read is refused by the portal allowlist with `not_available_in_portal`.

## Inventory

| Command | Class | Portal capability (backend) | Reason, if excluded |
|---|---|---|---|
| `eaeh init` | CLI | – | Creates the data directory and applies migrations: installation administration, needed before any portal can run. |
| `eaeh health` | Read | Settings → Health and audit chain (`GET /v1/health`, `/v1/health/details`) | |
| `eaeh serve` | N/A | – | Runs the HTTP API server process; the portal is itself a server process. |
| `eaeh setup` | CLI | – | Installation bootstrap, identity and credential repair, backup/reset and starting the portal itself: local-machine administration that must work before any portal exists, and it handles the administrator credential, which the portal never sees. |
| `eaeh portal` | N/A | – | Starts the portal. |
| `eaeh admin bootstrap` | CLI | – | Creates the first administrator and token: credential issuance must not go through a no-login browser UI. |
| `eaeh audit verify` | Read | Settings → Health and audit chain (`GET /v1/audit/verify`) | |
| `eaeh audit list` | Read | Activity / Audit (`GET /v1/audit`, paginated) | |
| `eaeh workflow start` | Action | Project → Start workflow (`POST /v1/workflows`, operator) | |
| `eaeh workflow run` | Action | Project → Start workflow: definitions, execution plan and readiness (`GET /v1/projects/{id}/plan`), start, worker start (`POST /portal/projects/{id}/worker`) | |
| `eaeh workflow watch` | Read | Workflow page (live polling) | |
| `eaeh workflow status` | Read | Workflow page: stages, current gate, failure, pending action (`GET /v1/workflows/{id}/detail`) | |
| `eaeh workflow resume` | Action | Workflow → Resume (`POST /v1/workflows/{id}/resume`, operator; audited `workflow.resume`) | |
| `eaeh workflow restart` | Action | Workflow → Restart from stage / Retry failed stage: preview, then confirm (`POST /v1/workflows/{id}/restart/preview`, `POST /v1/workflows/{id}/restart`) | |
| `eaeh workflow show` | Read | Workflow page: instance and transition history | |
| `eaeh workflow list` | Read | Workflows (filters: project, workflow, stage, date, status) (`GET /v1/workflow-queue`) | |
| `eaeh workflow cancel` | Action | Workflow → Cancel, with confirmation (`POST /v1/workflows/{id}/cancel`, operator) | |
| `eaeh workflow definitions` | Read | Start workflow (definition list) (`GET /v1/workflow-definitions`) | |
| `eaeh approval list` | Read | Approvals (`GET /v1/approval-queue`) | |
| `eaeh approval review` | Action | Approvals → Review for workflow documents and skills (approver) | **Standards** stay CLI-only (ADR-0028): their mandatory scope needs the CLI review. |
| `eaeh approval decide` | Action | Approval review → Approve / Request changes / Reject (`POST /v1/approvals/{id}/decision`, approver) | Same standards exception as `approval review`. |
| `eaeh token issue` | CLI | – | Credential issuance; tokens must never reach a browser. |
| `eaeh token list` | CLI | – | Credential administration. |
| `eaeh token revoke` | CLI | – | Credential administration. |
| `eaeh token rotate` | CLI | – | Credential administration. |
| `eaeh principal create` | CLI | – | People and roles. The portal's project registration creates or grants the people named in it, through the same service as `project init`, and nothing else. |
| `eaeh principal disable` | CLI | – | Destructive identity administration. |
| `eaeh principal grant` | CLI | – | Access administration. Registration grants project access as part of `project init`. |
| `eaeh principal set-teams` | CLI | – | Access administration. |
| `eaeh project init` | Action | Projects → Register existing project: browse, detect, review and edit, register (`/portal/fs`, `/portal/projects/inspect`, `/portal/projects/register`, admin) | |
| `eaeh project readiness` | Read | Project page → readiness; Start workflow → readiness per workflow | |
| `eaeh project create` | Action | Covered by Register existing project, which creates the project if needed | A project without a workspace can only be created with the CLI; the portal registers folders. |
| `eaeh project list` | Read | Projects (`GET /v1/dashboard`, `/v1/projects`) | |
| `eaeh project set-domain` | CLI | – | Changes which mandatory standards apply: governance administration. |
| `eaeh plugin list` | CLI | – | Plugin administration (ADR-0026 allowlist). |
| `eaeh plugin enable` | CLI | – | Plugin administration. |
| `eaeh plugin disable` | CLI | – | Plugin administration. |
| `eaeh plugin retire` | CLI | – | Plugin administration. |
| `eaeh knowledge source-create` | CLI | – | Knowledge administration (steward). Registration creates the project's sources. |
| `eaeh knowledge sources` | Read | Knowledge & agents → Knowledge sources (`GET /v1/knowledge/sources`) | |
| `eaeh knowledge ingest` | CLI | – | Reads arbitrary local files and changes governed knowledge. Registration ingests the documents chosen in the project folder. |
| `eaeh knowledge import` | CLI | – | Same reason as `knowledge ingest`. |
| `eaeh knowledge list` | Read | Knowledge & agents → Knowledge documents (`GET /v1/knowledge/documents`) | |
| `eaeh knowledge search` | N/A | – | Retrieval for agents and developers; the portal shows documents and citations. |
| `eaeh knowledge retire` | CLI | – | Destructive knowledge change. |
| `eaeh knowledge delete` | CLI | – | Destructive knowledge change. |
| `eaeh knowledge conflicts` | Read | API only (`GET /v1/knowledge/conflicts`, allowlisted for portal reads) | No screen yet; listed as a gap. |
| `eaeh skill register` | Action | Skills → Register skill files (steward; same registration service) | Registering an arbitrary file path stays CLI-only. |
| `eaeh skill list` | Read | Skills (`GET /v1/skills/catalog`) | |
| `eaeh skill sync` | Action | Skills → Register skill files (`POST /v1/skills/sync`, steward) and Scan now (`POST /v1/skills/discover`) | |
| `eaeh skill pending` | Read | Skills → pending approvals | |
| `eaeh skill set-status` | CLI | – | Deprecating or retiring an approved skill: administration. |
| `eaeh brd ingest` | CLI | – | Reads a local file into governed knowledge. Registration ingests the BRD chosen in the project folder. |
| `eaeh trace` | Read | Approval review → traceability links (`GET /v1/traceability`) | |
| `eaeh artifact status` | Read | Workflow and project pages → workspace documents (re-hashed on each read) | |
| `eaeh artifact sync` | CLI | – | Writes files into the repository on request. In the portal, approved documents are written automatically at approval (ADR-0027). |
| `eaeh artifact export` | CLI | – | Writes a copy to a local path chosen on the command line. The portal shows artifacts and versions (`GET /v1/artifacts/{id}`). |
| `eaeh auth login` | CLI | – | Stores a token locally; tokens never reach the browser. |
| `eaeh auth status` | CLI | – | Local credential store. The portal shows its own identities in Settings. |
| `eaeh auth use` | CLI | – | Local credential store. |
| `eaeh auth renew` | CLI | – | Local credential store. |
| `eaeh auth logout` | CLI | – | Local credential store. |
| `eaeh task list` | Read | Tasks (filters, paging; `GET /v1/task-queue`); Execution → hand-offs | |
| `eaeh task show` | Read | Task page (`GET /v1/tasks/{id}`) | The hand-off package (project data for the AI client) is not shown. |
| `eaeh task tool` | N/A | – | Called by AI clients through MCP during a hand-off. |
| `eaeh task validate` | N/A | – | Used by AI clients before they submit. |
| `eaeh task submit` | N/A | – | AI-client submission; EAEH validates it. A person does not submit agent output through the portal. |
| `eaeh mcp` | N/A | – | Stdio MCP server for AI clients. |
| `eaeh worker start` | Action | Started automatically when a workflow is started from the portal and AI work waits (`POST /portal/projects/{id}/worker`) | Explicit start and stop stay with the CLI. |
| `eaeh worker stop` | CLI | – | Stops a local process. |
| `eaeh worker status` | Read | Execution → workers; Integrations (`GET /v1/executions`, `/v1/integrations`) | |
| `eaeh worker run` | N/A | – | The worker process itself. |
| `eaeh worker retry` | Action | Task page → Retry AI hand-off (`POST /v1/tasks/{id}/retry`, operator) | |
| `eaeh worker history` | Read | Execution → attempts; Task page → attempts (`GET /v1/executions`) | |
| `eaeh worker log` | CLI | – | Full worker logs may contain local paths and client output. The portal shows the redacted detail excerpt recorded per attempt. |
| `eaeh review` | Action | Approval review page (document, versions, validation, decision) | |
| `eaeh agents` | Read | Knowledge & agents → Agents (`GET /v1/agents`) | |
| `eaeh tools` | Read | Knowledge & agents → Tools (`GET /v1/tools`) | |

## Functional areas (minimum coverage, §4)

| Area | Portal | Where |
|---|---|---|
| Projects | create (by registration), register, list, inspect | Projects, Register existing project, Project page |
| Workspaces | associate (registration), inspect | Project page (workspace, manifest, documents) |
| Workflows | list, start, inspect, resume, restart, cancel | Workflows, Start workflow, Workflow page |
| Tasks | inspect, retry (hand-off), re-execute (restart from stage) | Tasks, Task page |
| Requirements / HLD / LLD | generated by the workflow; inspect, versions, approve | Workflow page, Approval review, Project → artifacts |
| Skills | discover, inspect, validate, approve, reject | Skills (ADR-0028 to ADR-0030) |
| Knowledge | registered documents and sources | Knowledge & agents |
| Agents | definitions, capabilities, skills, tools | Knowledge & agents, Task page |
| Approvals | list, review, approve, reject, request changes | Approvals |
| Artifacts | browse, inspect, versions | Project → artifacts, Workflow page. Export stays CLI-only. |
| Execution | workers, hand-offs, attempts, failures, redacted log excerpts | Execution, Task page |
| Governance | gate verification, audit history, decisions | Workflow → governance, Activity / Audit |
| Integrations | execution mode, AI clients, client files, worker status | Execution → Integration |
