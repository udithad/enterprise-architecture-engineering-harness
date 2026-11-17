# External AI Task Context Delivery – Plan (DRAFT, awaiting approval)

Scope: Phases 1–3 (external-AI hand-off, MCP, coordinator). No direct provider calls, no model API billing, no fallback. No roadmap change. Nothing below is implemented yet.

## 1. Root cause (from the real failed workflow `01a1e921-de41-7505-ab94-2dd8e6b6981d`, read-only)

| Evidence | Finding |
|---|---|
| `external_executions` attempts 1–3 (`analyse`) | all `no_submission`; last messages: "returned about 102 KB … saved to a file … no tool can open that file"; `knowledge.read` (the BRD) also ~100 KB |
| `eaeh_task_get` (`crates/eaeh-cli/src/mcp.rs`) | returns the **entire** package in one MCP result, no paging |
| Package composition (81 KB JSON) | `data` 36.9 KB, `input` 31.9 KB, of which `input.inputs` and `input.outputs` are **byte-identical copies** (15.9 KB each); the contract the AI must follow (instructions, rules, output_schema/outline/example, tools, citable) is only ~11 KB |
| Worker session | `--tools ""` + only four `mcp__eaeh__*` tools allowed, so the client has no file/Bash tool by design; `MAX_MCP_OUTPUT_TOKENS=200000` is already exported yet Claude Code 2.1.326 still diverted the result to a file ⇒ cannot be relied on |
| Coordinator accounting | `no_submission` counts as a dispatch attempt (`max_attempts = 3`) ⇒ three attempts burned without the AI ever seeing its task; failure not distinguishable from "AI did not try" |
| `eaeh_task_tool knowledge.read` | same unbounded result problem for the BRD |

Architectural cause: delivery of context depends on the client's display limit and filesystem access; EAEH has no delivery protocol, no acknowledgement and no failure class for "the AI could not read its task".

## 2. Design

1. **Package manifest + sections (MCP, no files).** `eaeh_task_get` returns a manifest: `package_hash` (version), total bytes, and per section `{name, bytes, chunks, class: mandatory|reference}`. If the whole package is ≤ the inline limit it is also returned inline (small tasks unchanged: one call). New `eaeh_task_section {task_id, section, offset, limit}` returns one chunk: `{section, offset, next_offset, total_bytes, sha256_of_section, done, content}`; chunk size default 16 KB, max 32 KB (below the display limit, with margin), cut on UTF-8 boundaries, never truncating (`done` only when the last byte was served). Deterministic: the package is immutable per `package_hash`, so the same `(section, offset)` returns the same bytes after any worker/server restart.
2. **Sections** (mandatory ⇒ must be acknowledged): `instructions`, `rules`, `output_schema`, `output_outline`, `output_example`, `skill_output_schemas`, `allowed_tools`, `citable`, `input`, `feedback`. Reference (read on demand, retrievals recorded): `data` (untrusted retrieved knowledge), `context`, `skills`, `standards`.
3. **De-duplication.** `input` is stored once; identical copies of a value inside `input` (the `inputs`/`outputs` duplicate) are replaced by a reference marker `{"$same_as":"inputs"}` – lossless, documented, reversible by the section reader; the original package hash is unchanged (dedup is a delivery encoding).
4. **Large tool results** (`eaeh_task_tool`, e.g. `knowledge.read` of a 100 KB BRD) use the same mechanism: results above the inline limit are stored as a task blob (new table, content-addressed) and returned as `{paged:true, blob, total_bytes, sha256, first chunk, next_offset}`; `eaeh_task_blob {task_id, blob, offset, limit}` reads the rest. Nothing is truncated; the governed tool call is executed and audited once.
5. **Acknowledgement.** `eaeh_task_ack {task_id, package_hash}`: EAEH checks from its own delivery record that every mandatory section was served completely for this attempt and the hash is current; only then is the attempt `context_acknowledged`. In the task-scoped MCP server `eaeh_task_tool`, `eaeh_task_validate` and `eaeh_task_submit` refuse before the acknowledgement (clear message naming the missing sections). The core `external_submit` (CLI `eaeh task submit`, tests) is not gated.
6. **Failure classes.** Add `context_delivery_failed` and `ai_execution_failed`; `invalid_output` is reported as `result_validation_failed`; `no_submission` keeps its meaning (context acknowledged, nothing submitted). Classification at `conclude`: no acknowledgement ⇒ `context_delivery_failed` (retrieval error, partial or interrupted retrieval, client ended before reading); acknowledged and client/tool crash ⇒ `ai_execution_failed`.
7. **Attempt accounting.** `context_delivery_failed` does **not** consume a dispatch attempt (like `interrupted`) nor a correction attempt (submissions untouched); it is bounded by a separate counter `context_attempts` (`[execution.coordinator] max_context_attempts`, default 3, exponential backoff) after which the task stops `blocked` with the remedy; `eaeh worker retry` resets it. Idempotency: leases, chunk reads and ack are idempotent; a re-acknowledgement returns the same record.
8. **Prompt.** Worker prompt rewritten: manifest → read every mandatory section completely (`next_offset` until `done`) → ack → reference sections/blobs as needed → build → validate → submit; explicit instruction to report `EAEH_RESULT context_unreadable <reason>` instead of guessing.
9. **Size limits.** `max_package_bytes` (default 16 MiB, configurable): a package above it is refused when the hand-off is created with an explicit error naming the largest sections (never truncated, never silently dropped).
10. **Traces.** New table `external_deliveries` (task, execution/attempt id, package hash, package bytes, mandatory bytes, requests, bytes served, sections complete/total, status `pending|partial|complete|acknowledged|failed`, failure reason, timestamps). `external_executions` gains `package_bytes`, `delivery_status`, `retrieval_status`; the audit events `external_execution.start/finish` and `eaeh worker history/status` and `eaeh task show` show them.
11. **MCP launch.** The worker passes `--execution <exec_id>` to `eaeh mcp --task`, tying deliveries to the attempt; interactive clients use an `interactive` attempt id.

Migration `0008_task_context_delivery.sql` (additive). Allowed MCP tools for the worker session extended with the three new tools.

## 3. Reproduction and tests (isolated, no touch of the real workflow)

* Reproduce first on a throw-away installation: a hand-off with a ~100 KB package driven by a **real `eaeh mcp --task` process** and a test client that, like Claude Code, refuses results above a size limit and has no file tools. Before the change: it fails (`no_submission` ×3). After: succeeds.
* Tests: small package (inline), large (paged, hash of reassembled sections = package), oversized (> `max_package_bytes` refused with explanation; just-under paged correctly), missing file access (client with MCP tools only), partial retrieval (stops before the last chunk ⇒ ack refused, `context_delivery_failed`, no attempt consumed), interrupted retrieval (client dies mid-way ⇒ recoverable, same bytes after restart), large tool result paging, ack gating of submit, bounded context retries ⇒ blocked, idempotent ack, deduplication round-trip, successful submission end-to-end through the coordinator, trace records.

## 4. Decisions needed

1. Is the acknowledgement **enforced** (tool/validate/submit refuse before ack) or only recorded?
2. Chunk and inline limits: 16 KB chunk (max 32 KB), inline when the whole package ≤ 24 KB, hard cap 16 MiB.

## 5. Limits to document

* Only the Claude Code worker path is exercised end-to-end with a real client at acceptance; the model-side behaviour (does it read all chunks) is verified with a scripted client, and with real Claude Code only if you run the acceptance step.
* Reference sections are not forced to be read; the validator still enforces coverage.
