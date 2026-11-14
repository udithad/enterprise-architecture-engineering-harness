# Context, Prompts, Models, Knowledge and Skills

## Instruction precedence and trust

| Trust (highest first) | Source | May be instructions? |
|---|---|---|
| `system` | Platform prompt `system.agent-core` | ✔ |
| `enterprise_policy` | Approved, active, mandatory enterprise/domain standards (`eaeh-knowledge` only) | ✔ |
| `agent_instruction` | Agent's registered prompt template | ✔ |
| `skill` | Approved skills bound to the agent (`eaeh-skills` only) | ✔ |
| `task_input` | Authenticated task input, platform validation feedback | ✘ data |
| `untrusted` | Tool output, model output, retrieved content | ✘ data |

Enforcement is by type: `ContextItem::instruction` accepts only a `RenderedPrompt`
(constructible only by rendering a registered `PromptTemplate` with `TrustedText`
variables) at trust ≥ `skill`; untrusted content can only be added through
`ContextItem::untrusted(…, Untrusted<String>, …)`. The assembler places instructions in the
system message followed by a fixed security notice, and every data item in a
`<data key trust source>` block with `<data`/`</data` sequences neutralised.

## Context assembly (`ContextAssembler`)

Scope filtering (items from other projects are dropped), expiry, conflict detection
(same key, different content ⇒ higher trust wins, conflict recorded), ordering (trust,
priority, key), token budget (heuristic 4 chars/token; instructions must fit or the call
fails), secret redaction of data, snapshot (SHA-256 manifest of key/trust/source/memory/
content hash) persisted in `context_snapshots` and referenced from model usage and
artifact provenance. Memory kinds recorded: instruction, execution, workflow (Phase 1);
project, enterprise, agent are defined for Phase 2.

## Prompt management (`PromptRegistry`)

Templates (`prompts/*.toml`): id, semver version, kind (system/agent/task), description,
body with `{{var}}` placeholders, declared variables (required, max length). Validation:
every placeholder declared and every declaration used. Rendering: single pass; values cannot
introduce placeholders. Lifecycle: `draft → approved → retired`; exactly one approved
version per id; approving an older version is a rollback. Platform prompt files are loaded
at startup as approved (operator-controlled configuration); a changed file without a
version bump is rejected. Admin API for registration/approval is available via
`PromptRegistry` (no HTTP route yet — TD-05).

## Model management

`ModelDescriptor`: id, provider, version, capabilities, context window, max output, input/
output cost per Mtok (micros), quality tier, residency. Routing: filter by capabilities,
context, quality, residency, unit-cost ceiling; order preferred-first, then cheapest, then
highest quality, then id. `ModelClient`: budget `model_calls` up front; skip open circuits
and too-small context windows; hooks around request and response processing; retryable
errors ⇒ fallback to next candidate and circuit-breaker accounting; non-retryable ⇒ stop;
on success record `model_usage` (provider, model, **reported model version**, tokens,
cost, agent instance, correlation, context snapshot, prompt refs) + `model.invoked`, charge
`tokens` and `cost_micros`, redact response content. Embedding/reranking models,
evaluation/benchmarking and residency policy enforcement beyond routing filters are later
phases.

**Phase 2:** the opt-in `eaeh-provider-anthropic` adapter (ADR-0015) is the first production
provider; it is disabled by default and verified against a mock server only. Every call is
first checked by the `model.invoke` egress policy using the highest data classification in
the assembled context (`restricted` never goes to an external provider). Model usage records
skill and knowledge references. Tests use `eaeh_testkit::ScriptedModelProvider`.

Context items carry a classification (instructions `public`, task input `internal`, knowledge
excerpts their document's classification); `AssembledContext.max_classification` feeds the
egress decision.

## Knowledge and skills

Implemented in Phase 2 — see [knowledge-and-skills.md](knowledge-and-skills.md).
