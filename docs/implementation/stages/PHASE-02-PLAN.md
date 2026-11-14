# PHASE-02 – Plan: Enterprise Knowledge and Skills Management

| Field | Value |
|---|---|
| Phase | 2 – Enterprise Knowledge and Skills Management (master §29) |
| Date | 2026-11-12 |
| Baseline | Phase 1 implementation (uncommitted working tree on `1527c09`; 140/140 tests) |
| Status | **APPROVED** ("Phase 2 design approved", 2026-11-12): recommendations for P2-D1…P2-D5 adopted; P2-D6 (live API run) not authorised → mock-only; P2-D7 still open. Implemented — see `PHASE-02-COMPLETION.md` |

## 1. Inspection findings: what exists and what must be extended

| Phase 1 asset | Reuse in Phase 2 | Extension needed |
|---|---|---|
| `Scope` (tenant + project), authz boundary checks, scoped store queries | Hard tenant/project isolation for all knowledge queries | Knowledge scopes: enterprise / domain / project / team / workflow (ABAC attributes on projects and principals) |
| `TrustLevel::{EnterprisePolicy, Skill}`, `MemoryKind::{Enterprise, Project}`, knowledge hook points | Reserved in Phase 1 exactly for this phase | Governed constructors for standard and skill instructions; emit `before/after_knowledge_retrieval` |
| `ContextAssembler` (trust order, fencing, scope filter, budget, snapshots) | Unchanged core; snapshots already record provenance | Accept knowledge items with citations; mandatory standards always included |
| `PromptTemplate` rendering (`TrustedText` only) | Skill instruction templates use the same mechanism | Skill-scoped template rendering |
| `AgentDefinition.skills: Vec<String>` (opaque) | — | Becomes versioned skill references (`id@req`); runtime resolves approved, compatible skills |
| `ApprovalManager` (human, role, SoD, hash binding) | Approval of standards and skills, bound to content hash | New subject types `knowledge_document`, `skill` |
| `ValidationEngine` + `secret.leak` validator | Document quality gate on ingestion (quarantine on failure) | Document-specific validators (metadata, size, format) |
| `ModelProvider` trait, `ModelClient` (budgets, hooks, usage, fallback) | Production adapter plugs in unchanged | `EmbeddingProvider` trait; **egress policy check** (`model.invoke`) on data classification |
| `ToolRegistry` / `ToolGateway` | New read-only tool `knowledge.search` (governed, scoped, audited) | — |
| SQLite (bundled, **FTS5 compiled in** — verified in `libsqlite3-sys` build flags) | Lexical index without new infrastructure | Migration `0002`: knowledge, chunks, FTS5 virtual table, embeddings, skills |
| `SecretsProvider` + `Redactor` | Provider API key via `EAEH_SECRET_ANTHROPIC_API_KEY`, registered for redaction | — |
| Fitness tests (ADR-0002) | Extended with rules for the new crates | New boundary rules (below) |

Upstream status of Phase 1 carry-overs (checked today): `sqlx` latest is still 0.9.0 ⇒ **A-10 remains blocked** (E-1 stays). Phase 1 exceptions E-1/E-2 have not been explicitly accepted — see decision P2-D7.

## 2. Target design

### 2.1 New and changed crates

| Crate | New/changed | Responsibility | Allowed internal deps (fitness) |
|---|---|---|---|
| `eaeh-knowledge` | **new** | Sources, documents (versioned), parsing & chunking, classification & metadata, validation/quarantine, lifecycle, indexing (FTS5 + vectors), retrieval (lexical, vector, hybrid RRF, rerank), scopes/ACL, freshness, conflict detection, mandatory standards, citations, audit/events/hooks | core, store, events, governance, extensibility, validation, intelligence |
| `eaeh-skills` | **new** | Skill registry: versioned definitions, dependencies & compatibility, typed contracts, required knowledge & tools, permissions, validation, lifecycle with human approval, deterministic selection | core, store, events, governance, validation, intelligence |
| `eaeh-provider-anthropic` | **new** | Production `ModelProvider` adapter for the Anthropic Messages API (raw HTTPS; Rust has no official SDK) | core, intelligence (+ `reqwest`/rustls — only this crate may depend on an HTTP client) |
| `eaeh-intelligence` | changed | `EmbeddingProvider` trait + registry; built-in deterministic embedder; governed context constructors for standards/skills; egress policy hook in `ModelClient` | unchanged |
| `eaeh-agents` | changed | Resolve approved skills, retrieve knowledge for context, record skill/knowledge versions | + knowledge, skills |
| `eaeh-runtime`, `eaeh-api`, `eaeh-cli` | changed | Wiring, config, routes and commands for knowledge and skills | + knowledge, skills, provider-anthropic (runtime only) |

New fitness rules: governance and workflow still never depend on intelligence/knowledge/skills/agents; only `eaeh-knowledge` may construct enterprise-standard context items and only `eaeh-skills` skill-instruction items (source scan); only `eaeh-provider-anthropic` depends on an HTTP client; the provider crate is never a dependency of anything except `eaeh-runtime`.

### 2.2 Knowledge model

* **Source**: id, name, kind (`standard`, `reference`, `api_spec`, `adr`, `process`), owner, default scope and classification.
* **Document version**: source, subject key, version (monotonic), content hash, format, scope, classification (`public`/`internal`/`confidential`/`restricted`), tags, `mandatory` flag (standards), `review_by` date, status.
* **Lifecycle**: `ingested → validated | quarantined → pending_approval (standards) → active → superseded | retired → deleted` (content purged, tombstone + audit retained).
* **Ingestion**: inline content via API/CLI and local-directory import via CLI. Formats: Markdown (heading-aware chunks), plain text, JSON incl. OpenAPI 3 JSON (one chunk per operation/schema). **No URL fetching** (no SSRF surface). Size limits; quality gate (`secret.leak`, metadata, size) ⇒ quarantine on failure.
* **Scopes & ACL**: every query hard-filters tenant; visibility = enterprise ∪ domain(project's domain) ∪ project ∪ team(caller's teams / workflow initiator's teams) ∪ workflow(instance). Plus classification ceiling per principal/context.
* **Indexing**: SQLite FTS5 (BM25) over chunks; embeddings stored as f32 blobs; brute-force cosine within the visible set (documented scale limit).
* **Retrieval**: lexical, vector, hybrid (reciprocal rank fusion), deterministic reranker (field/heading match, authority: standard > reference, scope specificity, freshness). Query text is sanitised before FTS5 `MATCH` (no query-syntax injection). Results carry citations `doc@version#chunk` + content hash.
* **Freshness & conflicts**: stale (`review_by` passed) items flagged and optionally excluded; documents in the same subject with differing normative content in overlapping scopes recorded as conflicts; precedence enterprise > domain > project; **mandatory enterprise standards are always included and cannot be overridden** by project knowledge or retrieved content.
* **Standards in context**: approved mandatory standards enter at `enterprise_policy` trust (instructions); all other knowledge enters as **untrusted data** with citations.

### 2.3 Skills model

Skill definition (TOML/JSON, versioned): id, semver version, description, provided capabilities, applicable task kinds, instruction template (rendered at `skill` trust), required knowledge (queries/tags/sources), required tools, input/output schemas, permissions (max autonomy, max tool risk), dependencies (`id` + semver req), platform compatibility, evaluation criteria.

Lifecycle `draft → approved → deprecated → retired`; approval by a human Approver (SoD: registrant cannot approve), bound to the definition hash. Validation: dependency resolution and cycle check, tools exist, risk ≤ declared ceiling. **Binding rule**: a skill applies to an agent only if all its required tools are in the agent's allow-list and its autonomy ceiling ≤ the agent's — a skill can never grant tools or permissions. Selection is deterministic (task kind / capabilities, then highest approved compatible version). Skill versions used are recorded in agent results, model usage and artifact provenance.

### 2.4 Agent integration (RAG)

1. Before each agent run: resolve skills → retrieve required knowledge + task-query knowledge within the run's scope → assemble context: system prompt, mandatory standards, agent instruction, skill instructions, task input, knowledge excerpts (untrusted, cited).
2. During the run: read-only tool `knowledge.search` through the gateway (scope and classification enforced, audited).
3. After the run: deterministic **citation validator** — citations in the output must reference chunks actually retrieved in this run (fabricated citations fail the quality gate).

### 2.5 LLM provider integration (closes A-11)

* `eaeh-provider-anthropic`: `POST {base_url}/v1/messages`, headers `x-api-key`, `anthropic-version: 2026-10-21`; tools as `{name, description, input_schema}`, `tool_choice: auto`; **never sends `temperature`/`top_p`/`top_k`** (rejected by that API version); parses `text`/`tool_use` blocks, `usage`, `stop_reason` (`refusal` ⇒ non-retryable policy error; `max_tokens` ⇒ error); 429/5xx ⇒ retryable (`retry-after` honoured) for fallback; 4xx ⇒ non-retryable.
* Credentials only from the secrets provider (`EAEH_SECRET_ANTHROPIC_API_KEY`), registered for redaction; never in config, logs or audit; provider is **disabled unless explicitly enabled** in config.
* Model catalogue in config (id, pricing, context, residency). Default: `claude-opus-5-5` ($4/$20 per MTok, 1M context). Operators can add others (e.g. `claude-sonnet-5-5`, `claude-haiku-5-5`); note routing picks the cheapest model satisfying an agent's requirements.
* Refusal handling: the API's server-side refusal fallback (`fallbacks: "default"`, beta `server-side-fallback-2026-07-01`) enabled by default and configurable off.
* **Egress governance**: `ModelClient` evaluates policy action `model.invoke` with provider kind (`external`) and the highest classification in the assembled context. Structural baseline: `restricted` content is never sent to external providers (deny); enterprises add their own rules.
* Embeddings: no Anthropic embedding endpoint is assumed. `EmbeddingProvider` trait with a built-in **deterministic hashed n-gram embedder** (local, no egress, *not semantic* — documented as such); a semantic embedding provider is a later decision (P2-D2).

### 2.6 API / CLI

`/v1/knowledge/sources`, `/v1/knowledge/documents` (ingest, list, get, approve via approvals, retire, delete), `/v1/knowledge/search`, `/v1/knowledge/conflicts`, `/v1/skills` (register, list, get, deprecate/retire; approval via `/v1/approvals`). CLI equivalents incl. `eaeh knowledge import <dir>`. New role **`knowledge_steward`** (manage sources/documents; cannot approve).

### 2.7 Demonstration

* Fixture corpus `examples/knowledge/` (clearly labelled demonstration content, **not** platform standards): an illustrative API-design standard (mandatory), a reference guideline, and an OpenAPI spec to review.
* Skill `api-design-review@1.0.0`, approved by a human approver in the demo.
* Workflow `knowledge.review`: agent reviews the spec using the skill and retrieved standards, produces a cited review artifact, human approval gate.
* Runs (a) deterministically in CI as an e2e test with the scripted test double, and (b) via `scripts/demo-knowledge.sh` against the real server — with live Claude **only if you authorise it and provide a key** (P2-D6).

### 2.8 Persistence

Migration `0002_knowledge_skills.sql`: `knowledge_sources`, `knowledge_documents`, `knowledge_chunks`, `knowledge_chunks_fts` (FTS5), `knowledge_embeddings`, `knowledge_conflicts`, `skills`, `skill_dependencies`; columns for project `domain` and principal `teams`; `model_usage` + `skill_refs`, `knowledge_refs`. All writes via the serialized writer (fitness-enforced).

## 3. Implementation sequence (compile + test after each)

| Step | Deliverable |
|---|---|
| 0 | ADR-0012…0017 (knowledge architecture, retrieval & embeddings, skills governance, provider adapter & egress policy, knowledge scopes/ABAC, role `knowledge_steward`); dependency evaluation (reqwest/rustls, cargo-deny) |
| 1 | Migration 0002; scope/ABAC attributes; role |
| 2 | `eaeh-intelligence`: `EmbeddingProvider`, deterministic embedder, governed constructors, egress policy check |
| 3 | `eaeh-knowledge`: sources, parsing/chunking, validation/quarantine, lifecycle + approvals |
| 4 | Indexing & retrieval (FTS5, vectors, hybrid, rerank), freshness, conflicts, standards precedence, hooks/events/audit |
| 5 | `eaeh-skills`: registry, validation, deps, lifecycle + approvals, selection |
| 6 | Agent integration: skill resolution, RAG context, `knowledge.search` tool, citation validator, traceability |
| 7 | `eaeh-provider-anthropic` + runtime wiring; tests against a local mock HTTP server |
| 8 | API/CLI routes; demo corpus, skill, workflow, script |
| 9 | Security, e2e, retrieval-quality (recall@k on a fixed fixture set), fitness extensions |
| 10 | Docs, ADRs, roadmap, action register, test strategy, `PHASE-02-COMPLETION.md` |

## 4. Phase 2 acceptance criteria

| ID | Criterion |
|---|---|
| AC2-01 | Full gate (`scripts/ci.sh`) passes; all Phase 1 tests remain green |
| AC2-02 | Fitness rules for new crates pass (layering, governed constructors, HTTP client confinement) |
| AC2-03 | Sources registered; documents ingested (Markdown, text, JSON/OpenAPI) into versioned, chunked, classified records |
| AC2-04 | Validation quarantines failing documents (e.g. embedded secrets); quarantined content is never retrievable |
| AC2-05 | Lifecycle: supersede, retire, delete (content purged, audit retained) behave as specified |
| AC2-06 | Standards activate only after human approval bound to content hash; SoD enforced |
| AC2-07 | Lexical, vector and hybrid retrieval return ranked, cited results; FTS query injection is neutralised |
| AC2-08 | No cross-tenant / cross-project / out-of-scope (domain, team, workflow) leakage through search API, tool or context assembly |
| AC2-09 | Classification ceilings enforced; `restricted` content never sent to an external provider |
| AC2-10 | Mandatory enterprise standards always present at `enterprise_policy` trust and not overridable by project docs or injected content |
| AC2-11 | Freshness flags/excludes stale items; conflicts detected and recorded with precedence applied |
| AC2-12 | Skills: versioned, dependencies resolved, cycles rejected, contracts validated |
| AC2-13 | Skills usable only when approved; a skill cannot extend an agent's tools or autonomy |
| AC2-14 | Dynamic skill selection is deterministic; skill and knowledge versions recorded in results, model usage and artifact provenance |
| AC2-15 | Fabricated citations fail the citation validator |
| AC2-16 | Anthropic adapter: correct wire format (headers, version, no sampling params, tools), tool-use round trip, usage, refusal and error mapping, retry/fallback — verified against a mock server |
| AC2-17 | Provider credentials never appear in config, logs, audit, context or events; provider disabled unless explicitly enabled |
| AC2-18 | Knowledge hooks and events emitted; knowledge operations audited |
| AC2-19 | API and CLI for knowledge and skills: authenticated, authorized (steward / approver / viewer), paginated |
| AC2-20 | Demo: an agent uses approved skill + enterprise knowledge to produce a cited, approved artifact (CI, scripted) |
| AC2-21 | Retrieval-quality baseline (recall@k on fixed set) measured and recorded |
| AC2-22 | Documentation, ADRs, roadmap, action register, test strategy, completion report updated |
| AC2-23 | (Only if P2-D6 authorised) Live smoke run against the Anthropic API recorded |

## 5. Risks

| ID | Risk | Mitigation |
|---|---|---|
| R2-01 | Built-in embedder is not semantic ⇒ weaker vector recall | Hybrid with BM25; measured recall@k; pluggable `EmbeddingProvider` |
| R2-02 | Knowledge documents carry prompt injection | Untrusted data fencing; standards only via approval; tests |
| R2-03 | Data egress to an external LLM | Explicit enablement; classification-based egress policy; audit of every call |
| R2-04 | SQLite brute-force vector search scale | Documented limit; trait seam for a vector store later |
| R2-05 | E-1 (SQLite advisory) more relevant as more data is ingested | Unchanged mitigations; still blocked upstream |
| R2-06 | Live API cost / non-determinism | Live runs only with authorisation; CI uses mock server + scripted double |
| R2-07 | New HTTP/TLS dependencies (licences, advisories) | cargo-deny gate; confined to one crate |

## 6. Decisions requested

| ID | Decision | Recommendation |
|---|---|---|
| P2-D1 | Production model provider | Anthropic Messages API adapter (raw HTTPS), default model `claude-opus-5-5`, server-side refusal fallback on |
| P2-D2 | Embeddings | Built-in deterministic hashed embedder now; semantic embedding provider decided later |
| P2-D3 | Ingestion formats | Markdown, plain text, JSON/OpenAPI-JSON; defer YAML/PDF/DOCX and URL fetching |
| P2-D4 | Roles & approvals | New `knowledge_steward` role; standards and skills need human Approver approval with SoD |
| P2-D5 | Egress baseline | Deny `restricted` content to external providers; default classification `internal` |
| P2-D6 | Live API verification | Authorise a small live smoke run (you supply the key via `EAEH_SECRET_ANTHROPIC_API_KEY`); otherwise mock-only, reported as such |
| P2-D7 | Phase 1 exceptions | Confirm acceptance of E-1 (SQLite advisory) and E-2 (Podman/macOS-only isolation verification) |
