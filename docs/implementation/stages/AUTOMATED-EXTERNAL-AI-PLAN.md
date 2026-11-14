# Plan: Automated External AI Execution (builds on ADR-0025)

Status: **proposed — awaiting approval**. Nothing below is implemented yet.

## 1. Current state (inspected)

| Area | What exists | Where |
|---|---|---|
| Execution modes | `external_ai` (shipped default), `direct_provider`, `deterministic`; no provider registered in `external_ai` | `eaeh-agents/src/external.rs`, `eaeh-runtime/src/harness.rs` |
| Hand-off | Agent task builds a package, stores it in `external_tasks`, workflow suspends (`waiting_external`) | `external.rs` |
| Submission | `Harness::submit_external` validates (schema, skills, citations, validators), drives the workflow on; duplicate submit refused (`status = 'awaiting'` guard) | `harness.rs:621`, `external.rs:851` |
| Corrections | `request_changes` re-runs the producing state with comments, bounded by `max_revisions`; invalid submissions use the per-task correction budget | `eaeh-workflow/src/engine.rs:708` |
| Recovery | `engine.recover()` marks running tasks interrupted and re-drives running instances (only in `start_background`, i.e. `eaeh serve`) | `engine.rs:1900` |
| Client surfaces | MCP `eaeh_task_list/get/tool/submit`; CLI `eaeh task list/show/tool/submit` | `eaeh-cli/src/mcp.rs`, `dx.rs` |

**Where manual hand-off happens today.** Every time the workflow reaches `waiting_external` (BA analysis, requirements refinement, architect/HLD, LLD …) nothing runs until a person opens Claude Code and asks it to do the task. After every approval, the person again has to prompt the assistant. There is no process that watches for waiting tasks.

## 2. Verified Claude Code facts (2026-11-13, this machine)

- `claude --version` → `2.1.323`. Non-interactive mode `claude -p` with `--output-format stream-json`, `--mcp-config`, `--strict-mcp-config`, `--tools ""` (disable built-in tools), `--allowedTools`, `--permission-prompts none`, `--no-session-persistence`, `--model`, `--max-budget-usd`.
- `claude auth status --json` → `loggedIn: true`, `authMethod: "claude.ai"`, `apiProvider: "firstParty"`, `subscriptionType: "pro"`. Fields are machine-readable.
- A test run (`claude -p … --output-format stream-json --verbose`, haiku, no tools) reported `apiKeySource: "none"` in its `system/init` event, i.e. it used the claude.ai login, not an API key. The `result` event still reports a notional `total_cost_usd`; for a subscription login this is **not** a separate bill.
- `--bare` forces API-key authentication (it never reads OAuth/keychain) — **must never be used**.
- `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_USE_BEDROCK/VERTEX/FOUNDRY` or an `apiKeyHelper` would switch Claude Code to separately billed auth. None are set here.
- A GitHub Copilot CLI binary exists inside the VS Code extension storage; its headless mode and auth have **not** been verified.

**Assumption still to verify during implementation:** that a multi-turn headless run calling EAEH MCP tools keeps `apiKeySource: "none"` (expected: auth is per process, not per tool).

## 3. Target architecture

```
eaeh workflow run design ──► Workflow engine (unchanged, authoritative)
                                   │ suspends: waiting_external (task package stored)
                                   ▼
                     External Execution Coordinator  (new, eaeh-runtime)
                       • finds awaiting tasks, takes a DB lease
                       • preflight: provider available + subscription auth
                       • dispatches to ExternalExecutor (trait)
                       • bounded retries / timeout / cancel / classify failure
                       • records an execution row + audit per attempt
                                   │
                ┌──────────────────┴─────────────────┐
         ClaudeCodeCli adapter                CopilotCli adapter
         (implemented)                        (stub: "not supported", reports why)
                │  spawns: claude -p  (cwd = project root, scrubbed env)
                │   --mcp-config {eaeh mcp --task <id>}  --strict-mcp-config
                │   --tools ""  --allowedTools mcp__eaeh__eaeh_task_*  --permission-prompts none
                ▼
         Task-scoped EAEH MCP server (`eaeh mcp --task <id>`)
         exposes ONLY task_get / task_tool / task_submit for that one task
                │  submit → existing validation → engine drives on
                ▼
         next hand-off (coordinator picks it up) … or approval gate → stop + notify
                                   ▲
   eaeh approval decide … ─────────┘  (human only; engine resumes; coordinator continues)
```

Key properties:
- **One workflow engine.** The coordinator only executes `awaiting` hand-offs; it never changes workflow or approval state except through the existing `submit_external` path.
- **Claude Code has no file/shell tools in this path** (`--tools ""`). It can only read its task, call that task's allowed EAEH tools (governed, audited, as the agent principal) and submit. It cannot touch other tasks or projects (task-scoped MCP server, project/tenant checks unchanged).
- **No approval tool anywhere**; the coordinator's principal is the operator who started the run, so separation of duties already bars it from approving.
- **Fail closed on billing:** preflight requires `authMethod = claude.ai` and no API-key env; the env passed to `claude` is scrubbed of API-key/provider variables; the adapter aborts the attempt if the `init` event reports any `apiKeySource` other than `none`. No fallback to `direct_provider` or another provider, ever.

## 4. Components

| # | Change | Crate / file |
|---|---|---|
| 1 | Migration `0005`: `external_executions` (attempt rows: provider, client version, auth status, start/end, outcome, failure class, retry no., notional cost, exit code) and lease columns on `external_tasks` (`lease_owner`, `lease_expires_at`, `dispatch_attempts`, `next_attempt_at`, `dispatch_state`) | `eaeh-store` |
| 2 | `ExternalExecutor` trait + `ExecutionRequest/Outcome/FailureClass` (cli_unavailable, auth_unavailable, billing_unsafe, usage_limit, timeout, invalid_output, tool_failure, interrupted, crashed, cancelled) | `eaeh-runtime/src/coordinator/` |
| 3 | `ClaudeCodeCli` adapter: detect, version, `auth status`, spawn, stream-json parse, timeout (kill process group), cancel, output caps, redaction | same |
| 4 | Coordinator: poll + lease, bounded retries with exponential backoff (default 2 retries, never for auth/billing/usage-limit), per-task attempt cap, single concurrency by default (Claude Code is one session at a time), lease expiry → restart recovery, temp file cleanup | same |
| 5 | `eaeh mcp --task <id>`: task-scoped tool list | `eaeh-cli/src/mcp.rs` |
| 6 | Config `[execution.coordinator]`: `provider = "claude_code"`, `claude_path`, `model` (optional), `task_timeout_secs`, `max_retries`, `concurrency`, `notify` | `eaeh-runtime/src/config.rs`, `config/eaeh.toml` |
| 7 | CLI: `eaeh worker start/stop/status` (background coordinator, pid + lock file per data dir), `eaeh workflow run --follow`, `eaeh workflow watch` (live progress timeline), `eaeh review [<approval>]` (shows the document under review + the exact decide command, no export needed), `eaeh execution list` (attempt history), `eaeh ai status` (mode, provider, auth) | `eaeh-cli` |
| 8 | Readiness: new checks — execution provider found, subscription auth OK, billing-unsafe env absent | `eaeh-runtime/src/project.rs` |
| 9 | Recovery fix: `recover()` must not interrupt tasks owned by a live worker (guarded by the worker lock) | `engine.rs` / harness |
| 10 | MCP: manual `eaeh_task_*` tools kept (admin/troubleshooting); instructions in `CLAUDE.md` updated so an interactive assistant does not race the worker (it only acts on tasks with no live lease) | `mcp.rs`, `dx.rs` |

## 5. Developer experience after the change

```bash
eaeh workflow run design --yes        # starts the run and the background worker (if not running)
# … worker does BA → requirements; desktop notification: "Requirements ready for approval"
eaeh review --as approver             # shows requirements.md and the decide command
eaeh approval decide <id> --decision approve --comments "ok" --artifact-hash <h> --as approver
# … worker continues to HLD automatically; notification; review; approve; LLD; approve
eaeh workflow watch                   # optional live timeline
```

The user performs only the three document decisions.

## 6. Tests

- **Adapter (fake `claude` script on a temp PATH, labelled as test doubles):** not installed, not logged in, API-key env present → billing_unsafe, init `apiKeySource != none` → abort, success, timeout + kill, invalid/garbled output, usage-limit message, crash, cancellation, retry/backoff bounds, duplicate submission.
- **Workflow (in-process executor double that submits fixtures):** automatic analysis → refinement → requirements → pause; approve → HLD → pause; approve → LLD → pause; approve → succeeded; request_changes → automatic correction → new version; restart with expired lease → recovery; two projects concurrently → isolation; worker never decides approvals.
- **Security:** no provider registered in `external_ai`, no Anthropic URL/key in the execution path (env scrub asserted), task-scoped MCP refuses other task ids/projects, secrets redacted from logs/audit.
- **Real end-to-end (actual Claude Code, subscription):** `simple-springboot-api` copy, BRD → requirements → HLD → LLD with only the user's three approvals; evidence log of each `claude -p` attempt (init `apiKeySource`, duration, outcome), `model_usage` = 0 rows.

## 7. Risks and limits (to be documented)

- Pro plan usage limits: a full design run is several long tasks; a limit hit stops the task as `usage_limit` (no retry storm) and is resumable later with `eaeh worker start`.
- Headless `claude -p` behaviour may change between Claude Code versions; the adapter checks the version and the init event every time.
- Claude Code's terms apply to unattended use; EAEH runs it only on the user's machine, on the user's request, one task at a time.
- Copilot: interface only until its headless auth is verified.
- The notional `total_cost_usd` is recorded as "notional (subscription)", never presented as a charge.

## 8. Sequence

1. Migration + coordinator types + Claude adapter + task-scoped MCP (with adapter tests).
2. Worker/lease/recovery + CLI (`worker`, `--follow`, `watch`, `review`, `execution list`, `ai status`) + readiness.
3. Workflow/security tests; CI green.
4. Real end-to-end with Claude Code (user approves the three gates).
5. Documentation review of every guide + inventory; implementation report `AUTOMATED-EXTERNAL-AI-EXECUTION-AND-DEX-IMPROVEMENTS.md`; stop for review.
