# Enterprise Knowledge and Skills (Phase 2)

Decisions: ADR-0012 (knowledge store), ADR-0013 (retrieval), ADR-0014 (skills), ADR-0015
(provider & egress), ADR-0016 (scopes/ABAC), ADR-0017 (steward role).

## Components

| Crate | Responsibility | Key interfaces |
|---|---|---|
| `eaeh-knowledge` | Sources, versioned documents, parsing/chunking, validation & quarantine, lifecycle, approval application, FTS5 + vector indexing, lexical/vector/hybrid retrieval, reranking, scopes/ABAC, freshness, conflicts, mandatory standards, `knowledge.search` tool | `KnowledgeService`, `IngestRequest`, `RetrievalContext`, `SearchQuery`, `SearchHit`, `KnowledgeSearchTool` |
| `eaeh-skills` | Skill definitions, validation, dependencies/cycles, approval lifecycle, permission-preserving binding, deterministic resolution | `SkillRegistry`, `SkillDefinition`, `Binding`, `Resolution` |
| `eaeh-intelligence` (extended) | `EmbeddingProvider` + `HashedNgramEmbedder`; classification on context items; governed constructors `enterprise_standard` / `skill_instruction`; egress policy check in `ModelClient` | `EmbeddingProvider`, `ContextItem`, `ModelClient`, `Egress` |
| `eaeh-agents` (extended) | Runs skills + standards + RAG per agent execution; citation validator; traceability | `KnowledgeIntegration`, `validate_citations` |
| `eaeh-provider-anthropic` | Production `ModelProvider` (opt-in) | `AnthropicProvider`, `AnthropicConfig` |

## Data flow: agent run with knowledge and skills

```
AgentRuntime::execute
 ├─ SkillRegistry::resolve(explicit refs, allowed tools, autonomy, task kind)
 │     → approved, compatible skills (+ dependencies), skipped list
 ├─ KnowledgeService::context_for_agent(principal, scope, task) → RetrievalContext
 │     (tenant, project, project domain, workflow of task, ceiling = agent_max_classification)
 ├─ standards_context(ctx)  → MANDATORY ENTERPRISE STANDARD items  (enterprise_policy trust)
 ├─ resolution.instruction_items() → APPROVED SKILL items          (skill trust)
 ├─ search(task kind + input; each skill's required_knowledge)    → cited excerpts (untrusted data)
 └─ loop: ContextAssembler (system > enterprise_policy > agent > skill | data)
          → ModelClient: egress policy (model.invoke, classification) → provider
          → tool calls via gateway (incl. knowledge.search; its citations join the retrieved set)
          → final output: agent schema + every skill output contract + citation validator
 ⇒ AgentResult.skill_refs / knowledge_refs → model_usage, audit, artifact provenance
```

Instruction precedence in the system message (verified in `knowledge_agents.rs`):
platform system prompt → mandatory enterprise standards → agent instructions → skill
instructions → security notice. Retrieved content (including any injected instructions in a
document) appears only inside fenced `<data>` blocks.

## Knowledge lifecycle

| From | Event | To |
|---|---|---|
| — | ingest, validation fails | `quarantined` (no content stored, not indexed) |
| — | ingest, `standard` source | `pending_approval` (+ approval request bound to content hash) |
| — | ingest, other source | `active` (previous active version of same source/subject → `superseded`) |
| `pending_approval` | approver approves matching hash | `active` |
| `pending_approval` | approver rejects | `rejected` |
| `active` / `pending_approval` / `superseded` | steward retires | `retired` |
| any but `deleted` | steward deletes | `deleted` (content, chunk text, FTS rows, vectors purged) |

## Retrieval

* Visibility (SQL): `tenant_id = ctx.tenant AND status = 'active' AND classification_rank <= ceiling AND (enterprise OR domain = ctx.domain OR project = ctx.project OR team ∈ ctx.teams OR workflow = ctx.workflow)`.
* Modes: lexical (BM25), vector (cosine), hybrid (RRF). Candidates per mode: 50.
* Reranking: relevance first; heading match ×1.01; stale ×0.5 (and excluded unless `include_stale`); ties broken by source authority, mandatory flag, citation.
* Flags on hits: `stale`, `conflict`; classification; content hash; deterministic citation.
* Hooks `before/after_knowledge_retrieval` (veto possible); audit `knowledge.search`; event `knowledge.retrieved`.
* Baseline quality (fixed set, `crates/eaeh-knowledge/tests/retrieval_eval.rs`): recall@3 1.0 for all modes on lexically-overlapping queries; paraphrases: lexical 0.6, vector 0.6, hybrid 0.8.

## Skills

See ADR-0014. Selection is deterministic; a skill can never extend an agent's tools or
autonomy; skill input/output contracts are enforced; applied skill versions are recorded.

## Configuration (`config/eaeh.toml`)

```toml
[knowledge]
embedding_dims = 384           # built-in hashed n-gram embedder
retrieval_k = 4                # hits per automatic query
max_document_bytes = 1048576
chunk_chars = 1200
human_max_classification = "confidential"
agent_max_classification = "confidential"
exclude_stale_standards = false

[providers.anthropic]
enabled = false                # key only via EAEH_SECRET_ANTHROPIC_API_KEY
```

## Known limitations

| Id | Limitation |
|---|---|
| KS-L1 | Built-in embedder is lexical (non-semantic); paraphrase recall is limited (A-29). |
| KS-L2 | Brute-force vector scan over visible chunks; suitable for small/medium corpora (A-24 family). |
| KS-L3 | No YAML/PDF/DOCX ingestion, no connectors/URL fetching (P2-D3). |
| KS-L4 | Team-scoped knowledge is not visible to agents (A-31). |
| KS-L5 | Conflict detection is subject-key based (same subject, different content); semantic contradictions are not detected. |
| KS-L6 | Citation validator checks that citations were retrieved, not that the cited text supports the claim (Phase 5 evaluation). |
| KS-L7 | Anthropic adapter verified only against a mock server (A-30). |
