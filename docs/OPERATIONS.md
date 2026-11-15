# EAEH Operations Guide (Phases 1–3)

## Prerequisites

| Requirement | Version used for Phase 1 evidence |
|---|---|
| Rust toolchain | 1.99.1 (installed automatically from `rust-toolchain.toml` by rustup) |
| Podman **or** Docker (for `exec.run` / container isolation) | Podman 5.5.1 (`podman machine` on macOS) |
| Container image | `docker.io/library/alpine:3.20` pre-pulled (`podman pull docker.io/library/alpine:3.20`) |
| Optional: `cargo-deny` | 0.20.2 (`cargo install --locked cargo-deny`) |
| For smoke script: `curl`, `python3` | system |

## Build and verify

```bash
cargo build --workspace --locked            # binary: target/debug/eaeh
scripts/ci.sh                               # fmt, clippy -D warnings, build, all tests, cargo-deny
EAEH_SKIP_CONTAINER_TESTS=1 scripts/ci.sh   # explicitly skip container tests (reported as SKIPPED)
scripts/smoke-api.sh                        # end-to-end HTTP smoke test against `eaeh serve`
```

## Configure

`config/eaeh.toml` (paths relative to the file). Sections: `storage` (`data_dir`),
`definitions` (prompts/agents/workflows/policies directories), `isolation` (process and
container providers, runtime, image, uid, `exec_allowed_programs`, limits), `plugins`
(`require_signature`, `trusted_keys` hex ed25519, `grantable`), `api` (`bind`,
`max_body_bytes`, `requests_per_minute`), `supervision`, `models`, `log` (`json|pretty`,
filter), and (Phase 2) `knowledge` (embedding dimensions, `retrieval_k`, size and chunk
limits, classification ceilings for humans and agents, `exclude_stale_standards`) and
`execution` (`mode = external_ai | direct_provider | deterministic`, ADR-0025; absent = `direct_provider`; the shipped file sets `external_ai`),
`providers.anthropic` (`enabled`, `base_url`, `api_version`, `timeout_secs`, `max_retries`, `refusal_fallback`, `effort`, `models`), and (Phase 3) `requirements` (analyzer `weak_phrases`, `unresolved_markers`, `nfr_categories`, similarity thresholds; all optional). Unknown keys are rejected. **Never put credentials in the file**; secrets are read
from `EAEH_SECRET_<KEY>` environment variables via the secrets provider. Override the file
with `--config` or `EAEH_CONFIG`.

On macOS with Podman/Docker Desktop, keep `storage.data_dir` under your home directory so
workspaces can be mounted into containers.

## First start (credential provisioning)

```bash
B=target/debug/eaeh
$B init                                                    # creates data dir (0700), DB (0600), migrations, loads definitions
$B admin bootstrap --tenant acme --project platform \
   --admin-id admin --token-file ./admin.token             # one time only; token file mode 0600
$B auth login --profile admin --token-file ./admin.token && rm ./admin.token   # stored 0600, never printed
# then, per project, one command (see docs/user-guides/EAEH-USER-GUIDE.md):
$B project init --name <project> --developer <id> --steward <id> --approver <id> --brd <BRD.md> …
# or manually:
$B principal create --id arthur --display-name Arthur --roles approver --projects <project-id>
$B token issue --principal arthur --label laptop --token-file ./arthur.token   # hand over; never printed
```

The CLI authenticates with a stored profile (`eaeh auth login|status|use|renew|logout`,
`--as <profile>`) or with `EAEH_TOKEN`. `token issue` and `token rotate` require a destination:
`--token-file` (0600), `--store-profile <name>` or `--print` (displays the secret; avoid).
Revoke: `eaeh token revoke <token-id>`; list: `eaeh token list [--principal id]`. Disable a
principal: `eaeh principal disable <id>`; give an existing principal access to a project:
`eaeh principal grant --id <id> [--project <id>]`.

## Run, stop

```bash
$B serve        # API on api.bind (default 127.0.0.1:8080), event dispatch, supervision, recovery
# stop: Ctrl-C (SIGINT) → graceful shutdown: stops background loops, drains events, closes DB
```

Interrupted workflows are recovered automatically on the next `serve`.

## CLI reference

| Command | Auth | Purpose |
|---|---|---|
| `init`, `health` | local | initialise / report health |
| `admin bootstrap …` | local, one-time | first tenant/project/admin + token |
| `audit verify` | local | verify the audit hash chain (exit code 6 if invalid) |
| `audit list` | Auditor/Admin | list audit records for the tenant |
| `serve` | — | run the API server |
| `workflow definitions | start | show | list | cancel` | Operator/Viewer… | workflows (`start` runs until completion or approval wait) |
| `approval list | decide <id> --decision approve|reject|request_changes --artifact-hash H [--comments]` | Approver | decide gates (`request_changes`: workflow gates only, comments required) |
| `brd ingest --file F.md --project P [--source brd] [--subject S] [--classification confidential]` | Steward/Admin | ingest a BRD as project-scoped knowledge (tag `brd`); prints its citation `source/subject@vN` |
| `workflow start --definition engineering.design --project P --input '{"brd":"brd/order-management@v1"}'` | Operator | BRD → requirements → HLD → LLD (requires an enabled model provider and approved Phase 3 skills) |
| `trace --project P [--id ITEM] [--depth 3]` | Viewer+ | traceability links of approved artifacts |
| `artifact export --project P <artifact-id> --out DIR` | Viewer+ | write an artifact; a bundle manifest exports every file (hash-checked) |
| `token issue | list | revoke | rotate` | self / Admin | tokens |
| `principal create | disable`, `project create | list` | Admin | identity |
| `plugin list | enable | disable | retire <id> <version>` | Admin | plugins (effective next start) |
| `agents`, `tools` | Viewer+ | registries |
| `auth login --profile P [--token-file F] [--default]`, `auth status`, `auth use P`, `auth renew --profile P`, `auth logout --profile P [--revoke]` | own | credential profiles (0600 store in `$EAEH_HOME`, default `~/.config/eaeh`); tokens never printed |
| global `--as <profile>` (env `EAEH_PROFILE`), `--json` | – | choose identity; machine-readable output of developer-experience commands |
| `project init --name … --developer … --steward … --approver … [--brd] [--knowledge] [--standard] [--constraint] [--mode external_ai\|direct_provider\|deterministic] [--client claude-code\|github-copilot]… [--build-verification required\|deferred] [--build-program P] [--build-arg A]… [--workflow W]… [--store-profiles]` | Admin | one-command, idempotent project bootstrap incl. `CLAUDE.md`/`.mcp.json` (Claude Code) and `.github/copilot-instructions.md`/`.vscode/mcp.json` (GitHub Copilot); `--provider` is a deprecated alias of `--mode` |
| `project readiness [--workflow W]` | any | Ready/Not Ready with fixes (exit 7 if not ready) |
| `skill sync`, `skill pending` | Steward/Admin; any | register required skills from `definitions.skills_dir`; their states |
| `approval review [items…] [--all] [--yes] [--reject]` | Approver (explicit `--as`) | review queue for skills/standards; per-item confirmation |
| `workflow run requirements\|design\|deliver [--yes]`, `workflow status [id]`, `workflow resume <id>` | Operator; any | activity-based execution with prerequisite checks and next actions (`deliver` aliases: `plan`, `implement`, `test`) |
| `task list [--all]`, `task show <id> [--out F]`, `task tool <id> <tool> --input JSON\|@file`, `task submit <id> --output F\|- [--client C]` | list: any reader; others: Operator (`task.execute_external`) | external AI hand-offs (ADR-0025): package, governed tool calls as the agent, submission with validation |
| `artifact export <id> --out DIR` | Viewer+ | export an artifact or bundle (hash-checked); reports whether exactly this version was approved at a gate |
| `principal grant --id P [--project]` | Admin | add project access (roles unchanged) |
| `mcp [--project-root DIR]` | as profile | MCP stdio server for Claude Code and GitHub Copilot (VS Code); hand-off tools, no approval tool |
| `knowledge source-create --name --kind standard\|reference\|api_spec\|adr\|process --description`, `knowledge sources` | Steward/Admin; read: Viewer+ | knowledge sources |
| `knowledge ingest --file F --source S --project P [--subject] [--title] [--scope enterprise\|domain:<d>\|project:<id>\|team:<t>\|workflow:<id>] [--classification public\|internal\|confidential\|restricted] [--mandatory] [--review-by DATE]` | Steward/Admin | ingest one Markdown/text/JSON/OpenAPI-JSON file (standards → approval request) |
| `knowledge import DIR --source S --project P [--scope] [--classification] [--mandatory]` | Steward/Admin | ingest every `.md`/`.txt`/`.json` in DIR (subject = file stem) |
| `knowledge list [--status]`, `knowledge conflicts` | Viewer+ | documents, conflicts |
| `knowledge search --project P --query Q [--mode lexical\|vector\|hybrid] [--k N]` | Viewer+ | cited, scope-filtered search |
| `knowledge retire <id>`, `knowledge delete <id>` | Steward/Admin | lifecycle (delete purges content) |
| `skill register --file F --project P`, `skill list`, `skill set-status <id> <version> approved\|deprecated\|retired` | Steward/Admin; read: Viewer+ | skills (registration requests human approval) |
| `project set-domain <id> --domain D\|none`, `principal set-teams <id> --teams a,b` | Admin | ABAC attributes for knowledge scopes |

Exit codes: 0 ok, 1 error, 3 unauthenticated, 4 forbidden/policy denied, 5 not found, 6 integrity failure.

## HTTP API v1 reference

All routes except `GET /v1/health` need `Authorization: Bearer <token>`. Errors:
`{"error":{"code","message"}}`; every response has `x-request-id`. Lists accept `limit`
(1–100) and `cursor` and return `next_cursor`.

| Method & path | Permission | Notes |
|---|---|---|
| `GET /v1/health` | public | `{"status","api_version"}` |
| `GET /v1/health/details`, `GET /v1/metrics` | metrics.read | details JSON / Prometheus text |
| `GET /v1/me` | any | caller identity |
| `POST /v1/workflows` `{definition, project_id, input}` | workflow.start | 202; runs asynchronously |
| `GET /v1/workflows?project_id=` / `GET /v1/workflows/{id}` / `POST …/{id}/cancel` | workflow.read / cancel | instance + history |
| `GET /v1/workflow-definitions` | workflow.read | |
| `GET /v1/approvals?project_id=&status=` / `GET /v1/approvals/{id}` | approval.read | |
| `POST /v1/approvals/{id}/decision` `{decision: approve|reject|request_changes, comments, artifact_hash}` | approval.decide | human approver, not author; `request_changes` only for workflow gates, comments required |
| `GET /v1/traceability?project_id=&id=&depth=` | workflow.read | links recorded from approved artifacts; `id` filters with transitive closure (depth 1–10) |
| `GET /v1/artifacts/{id}?project_id=` | artifact.read | metadata + content, hash-verified |
| `GET /v1/agents`, `GET /v1/tools` | agent.read / tool.read | |
| `GET /v1/audit?after_seq=&limit=`, `GET /v1/audit/verify` | audit.read | tenant-filtered |
| `POST /v1/tokens` `{principal_id?, label, ttl_days?}`, `GET /v1/tokens`, `POST /v1/tokens/{id}/revoke`, `POST /v1/tokens/{id}/rotate?ttl_days=` | token.manage_own/any | tokens shown once (`Cache-Control: no-store`) |
| `POST /v1/principals`, `POST /v1/projects`, `GET /v1/projects` | principal.manage | |
| `GET /v1/plugins`, `POST /v1/plugins/{id}/{version}/{enable|disable|retire}` | plugin.manage | |
| `POST /v1/knowledge/sources`, `GET /v1/knowledge/sources` | knowledge.manage / read | |
| `POST /v1/knowledge/documents` `{governance_project, source, subject, title, format, content, scope, classification?, mandatory?, review_by?, tags?}`, `GET /v1/knowledge/documents?status=` | knowledge.manage / read | standards return `pending_approval` + approval id |
| `GET /v1/knowledge/documents/{id}`, `DELETE /v1/knowledge/documents/{id}`, `POST /v1/knowledge/documents/{id}/retire` | knowledge.read / manage | delete purges content |
| `POST /v1/knowledge/search` `{project_id, text, mode?, k?, kinds?, include_stale?}` | knowledge.read | hits with citation, score, hash, stale/conflict flags |
| `GET /v1/knowledge/conflicts` | knowledge.read | |
| `POST /v1/skills` `{project_id, definition}`, `GET /v1/skills`, `POST /v1/skills/{id}/{version}/{deprecate|retire|reinstate}` | skill.manage / read | approval via `/v1/approvals` |
| `POST /v1/projects/{id}/domain`, `POST /v1/principals/{id}/teams` | principal.manage | ABAC attributes |

## Model providers

**Execution modes (ADR-0025).**

| Mode | What happens to `agent_task` states |
|---|---|
| `external_ai` (shipped default) | Handed off to the connected AI client (`eaeh task …` / MCP `eaeh_task_*`). **No model provider is registered**, even if `[providers.*]` are enabled. No EAEH AI key is needed. `planned` states (`engineering.task`) need a model and are reported as not supported. |
| `direct_provider` (default when `[execution]` is absent) | Run by EAEH's model loop with the enabled providers. Without a provider they fail with `unavailable`. |
| `deterministic` | Only `[providers.replay]` is registered; it is required. |

`demo.approval` runs in every mode.

Anthropic adapter (Phase 2, ADR-0015; `direct_provider` mode only). It **sends task data and retrieved knowledge to an
external service**; enable it only after a data-protection review:

```bash
# config/eaeh.toml: [execution] mode = "direct_provider" and [providers.anthropic] enabled = true
export EAEH_SECRET_ANTHROPIC_API_KEY=...      # never put the key in the config file
target/debug/eaeh serve                       # start-up fails if enabled without the secret
```

Restricted-classified content is never sent (policy `model-restricted-egress-deny`). The
adapter has been verified against a local mock server only; no live run has been performed.

## Deterministic replay (not AI)

```toml
[providers.replay]
enabled = true
cassette = "../examples/simple-springboot-api/replay-cassette.json"
```

Returns recorded responses instead of calling a model (ADR-0023): local, zero cost, reported
model version `replay-cassette-<hash>`. For acceptance scenarios, demos and CI – never present
replayed output as AI-generated. `scripts/demo-onboarding.sh` uses it.

## Requirements and architecture design (Phase 3)

Prerequisites:
1. A steward ingests the BRD with `eaeh brd ingest` and the organisation's standards. Approvers approve the standards.
2. The steward registers the skills `config/skills/{requirements-analysis,hld-design,lld-design}.json`. Approvers approve them; the agents refuse to run with unapproved skills.
3. A model provider is enabled (see *Model providers*).

Running and reviewing:
- **Start:** run `engineering.design` (or `requirements.analysis`) with input `{"brd": "<citation>"}`.
- **At each gate:** review the exported bundle (`eaeh artifact export`), then approve, `request_changes` with comments, or reject.
- **Approved artifacts:** each one records trace links (`eaeh trace`).

`scripts/demo-design.sh` runs this flow on a real server with the labelled demonstration BRD (`examples/brd/`). It performs one request-changes round on the requirements and exports every bundle to `target/demo-design/export/`.
- **Without a provider:** the workflow stops at the first agent step with `unavailable`, and the script reports this.
- **Live:** run with `EAEH_DEMO_LIVE=1 EAEH_ANTHROPIC_KEY_FILE=<0600 file> scripts/demo-design.sh`. This is billed and capped by budgets: $5 per agent run and $25 per workflow.
- **Deterministic run** (scripted model): `cargo test -p eaeh-acceptance --test design_demo`.

## Knowledge demonstration

`scripts/demo-knowledge.sh` drives a real server: steward ingests `examples/knowledge/`,
approver approves the standard and the `api-design-review` skill, operator searches and
starts `knowledge.review`. Without a provider the agent step fails with `unavailable`
(reported as such); with `EAEH_DEMO_LIVE=1` and the secret set it runs live. The
deterministic agent run is `cargo test -p eaeh-acceptance --test knowledge_demo`.

## Backup and restore

See [architecture/data.md](architecture/data.md#migrations-backup-restore).
