# PHASE-02 – Completion Report: Enterprise Knowledge and Skills Management

| Field | Value |
|---|---|
| Phase | 2 – Enterprise Knowledge and Skills Management (master §29) |
| Date | 2026-11-12 |
| Plan | `PHASE-02-PLAN.md` — approved ("Phase 2 design approved"); P2-D1…P2-D5 per recommendation; P2-D6 not authorised; P2-D7 open |
| Baseline | `1527c09` + uncommitted Phase 1 work (140/140 tests) |
| Result | Implementation complete; **181/181 automated tests pass**; quality gate green |
| Exit decision | **READY WITH ACCEPTED EXCEPTIONS (E-1…E-4) — accepted by the product owner 2026-11-12**; Phase 3 authorised the same day |
| Version control | **Nothing committed or pushed** (A-27) |
| Phase 3 | **Not started** |

## 1. Summary

Phase 2 adds governed enterprise knowledge, retrieval-augmented agent execution, a governed
skills registry and the first production LLM provider adapter on top of the Phase 1
foundation, reusing its approval manager, policy engine, authorizer, serialized writer,
audit chain, outbox events, hooks, context assembler, tool gateway and secrets/redaction.
No Phase 1 architectural decision was changed; ADR-0012…0017 record the new decisions.

What works, verified by automated tests:

* Knowledge sources and immutable, versioned documents (Markdown, text, JSON, OpenAPI-JSON),
  chunked and classified; failing documents quarantined **without storing content**.
* Standards activate only after a human approver approves the exact content hash; the
  knowledge steward cannot approve (SoD). Supersede, retire and delete (purge) lifecycle.
* Lexical (FTS5/BM25), vector (hashed n-gram) and hybrid (RRF) retrieval with deterministic
  citations; scope (enterprise/domain/project/team/workflow), tenant and classification
  filtering in SQL; freshness and conflict flags; FTS query injection neutralised.
* Skills: versioned, digest-bound human approval, dependency resolution, cycle rejection,
  input/output contracts, deterministic selection; a skill can never extend an agent's tools
  or autonomy.
* Agents receive mandatory standards (`enterprise_policy` trust), approved skills (`skill`
  trust) and retrieved knowledge (untrusted, fenced, cited); can call `knowledge.search`
  through the gateway; fabricated citations fail validation; skill and knowledge references
  are recorded in agent results, model usage, audit and artifact provenance.
* `model.invoke` egress policy: restricted content never reaches an external provider.
* `eaeh-provider-anthropic`: opt-in Messages API adapter, key only from the secrets
  provider, verified against a local mock HTTP server.
* `/v1` routes and CLI commands for knowledge, skills and ABAC attributes; demonstration
  corpus, skill, workflow (`knowledge.review`) and script.

What is **not** done: a live run against the Anthropic API (not authorised — AC2-23); a
semantic embedding model; see §7.

## 2. Implementation details

### 2.1 Crates and components

| Crate | Status | Size (lines, src / tests) | Content |
|---|---|---|---|
| `eaeh-knowledge` | new | 1968 / 1125 | `model`, `parse` (chunking), `service` (ingest, approval application, lifecycle, contexts, conflicts, standards), `search` (FTS5, vectors, RRF, rerank, hooks, audit, events), `tool` (`knowledge.search`) |
| `eaeh-skills` | new | 729 / 443 | `SkillDefinition`, `SkillRegistry` (register, approve, status, resolve), `Resolution` |
| `eaeh-provider-anthropic` | new | 388 / 221 | `AnthropicConfig`, `AnthropicProvider` (wire format, tool-name mapping, retries, error mapping) |
| `eaeh-core` | changed | — | `Classification`; ids `KnowledgeSourceId`, `KnowledgeDocumentId`, `SkillId`; role `KnowledgeSteward` |
| `eaeh-governance` | changed | — | actions `knowledge.manage/read`, `skill.manage/read`; approval subjects `KnowledgeDocument`, `Skill`; baseline rules `model-invoke-allow`, `model-restricted-egress-deny`; project domain / principal teams |
| `eaeh-store` | changed | migration `0002_knowledge_skills.sql` (99 lines) | knowledge tables + FTS5, skills, ABAC columns, `model_usage` refs |
| `eaeh-events` | changed | — | `knowledge.ingested/activated/retrieved/conflict_detected`, `skill.registered/state_changed` |
| `eaeh-intelligence` | changed | — | `EmbeddingProvider` + `HashedNgramEmbedder`; `Egress`; classification on context items; governed constructors; egress check per routing candidate |
| `eaeh-agents` | changed | — | `KnowledgeIntegration`, skill resolution, RAG, `validate_citations`, traceability |
| `eaeh-workspace` | changed | — | `Provenance.skills`, `Provenance.knowledge` |
| `eaeh-runtime`, `eaeh-api`, `eaeh-cli` | changed | — | config, wiring, routes, commands |

Architecture: `docs/architecture/knowledge-and-skills.md` (new), plus updates to
`README.md`, `components.md` (#12, #14, #15, #16), `intelligence.md`, `security.md`,
`data.md`, `integrations.md` and `security/THREAT-MODEL.md` (T28–T40).

### 2.2 Decisions (ADRs)

| ADR | Decision |
|---|---|
| ADR-0012 | Governed knowledge store in SQLite: immutable versions, quarantine without content, approval for standards, purge on delete |
| ADR-0013 | Hybrid retrieval (BM25 + hashed vectors, RRF k=60), relevance-first reranking, built-in non-semantic embedder behind `EmbeddingProvider` |
| ADR-0014 | Skills as governed, permission-preserving capabilities; digest-bound approval; deterministic binding |
| ADR-0015 | Anthropic Messages API adapter (raw HTTPS, `anthropic-version: 2026-10-21`, no sampling params, server-side refusal fallback), credentials via secrets provider, egress policy; reqwest/rustls(ring) confined to one crate; `CDLA-Permissive-2.0` allowed |
| ADR-0016 | Knowledge scopes, ABAC attributes (project domain, principal teams), classification ceilings |
| ADR-0017 | `knowledge_steward` role; approval SoD for standards and skills |

### 2.3 New dependencies

`reqwest 0.13.5` (`rustls-no-provider`, `json`; no native-tls/OpenSSL) and `rustls 0.23.46`
(`ring`), used only by `eaeh-provider-anthropic` (fitness-enforced); `axum` reused as
dev-dependency for the mock server. `cargo deny check`: advisories, bans, licences, sources
ok (E-1 ignore unchanged; `CDLA-Permissive-2.0` added for `webpki-roots` CA data).

### 2.4 Changes to Phase 1 code and tests (disclosed)

* Call sites updated for new signatures (`ModelClient::new` takes the policy engine;
  `ModelCall` has `max_classification`, `skill_refs`, `knowledge_refs`); migration
  assertions changed from `[1]` to `[1, 2]`; expected agent/workflow lists include
  `api-reviewer` and `knowledge.review`.
* **One Phase 1 test was modified for robustness**:
  `process_isolation.rs::timeout_kills_entire_process_group` uses a 2 s timeout instead of
  500 ms because shell start-up occasionally exceeded 500 ms under parallel test load (the
  assertion that the whole process group is killed is unchanged).
* No Phase 1 test was removed or ignored.

## 3. Demonstration: agent using enterprise knowledge and an approved skill

**Deterministic (CI):** `tests/tests/knowledge_demo.rs::agent_reviews_api_spec_with_approved_skill_and_enterprise_knowledge`
— full harness with shipped configuration; the model is the labelled test double
`ScriptedModelProvider`. Verified assertions:

1. Steward ingests `examples/knowledge/`; the standard is `pending_approval` until the
   approver approves its content hash.
2. Steward registers `examples/skills/api-design-review.json`; approver approves it.
3. Workflow `knowledge.review` (agent `api-reviewer`) reaches `waiting_approval`; the
   review artifact's citations equal the agent output's; artifact provenance contains skill
   `api-design-review@1.0.0`, the standard, both guides and the spec chunk retrieved through
   `knowledge.search`.
4. The first model request contains `MANDATORY ENTERPRISE STANDARD [demo-api-standards/error-format@v1]`
   and `APPROVED SKILL [api-design-review@1.0.0]` in the system message and knowledge as
   `<data key="kb:…">` blocks.
5. Human approval of the artifact hash completes the workflow (`succeeded`);
   `model_usage` records the skill and knowledge refs; the audit chain is valid.

**Real server (no live model):** `scripts/demo-knowledge.sh` → `evidence/phase-02-knowledge-demo.log`.
Steward ingestion, steward approval attempt **403**, approver approval **200**, skill
approved, cited operator search, audit chain valid (47 records), 0 key/token literals in the
server log. The `knowledge.review` workflow **fails at the agent step** with
`unavailable: no registered model satisfies the routing requirements`, because no provider
is enabled (live calls not authorised). This is the expected, reported outcome — not a pass
of the live path.

The demonstration corpus is clearly labelled illustrative content, not enterprise standards
(A-09).

## 4. Test evidence

| Evidence | Produced by | Result |
|---|---|---|
| `evidence/phase-02-ci.log` (2026-11-12T17:07:25Z) | `scripts/ci.sh`: fmt, clippy `-D warnings`, build, `cargo test --workspace --locked --no-fail-fast`, `cargo deny check` | **181 passed, 0 failed, 0 ignored** (38 test binaries + 19 doc-test runs with 0 doc-tests); `==> CI PASSED` |
| `evidence/phase-02-retrieval-eval.log` | `cargo test -p eaeh-knowledge --test retrieval_eval -- --nocapture` | see §4.2 |
| `evidence/phase-02-knowledge-demo.log` (16:52:29Z) | `scripts/demo-knowledge.sh` | DEMO COMPLETE (live model: false) |
| `evidence/phase-02-api-smoke.log` (16:59:48Z) | `scripts/smoke-api.sh` (Phase 1 smoke, regression) | SMOKE PASSED; migrations `[1, 2]` |

The demo and smoke logs predate the final CI run by minutes; between them only a test was
added (`knowledge_hooks_observe_and_can_veto_retrieval`); no production code changed.
Earlier full runs during implementation were also green (180/180 three times before that
test was added). Container isolation tests ran against Podman 5.5.1 on macOS (E-2).

### 4.1 Test counts

| | Phase 1 | Added in Phase 2 | Total |
|---|---|---|---|
| Automated tests | 140 | 41 | **181** |

Phase 2 tests by suite: knowledge integration 10, knowledge unit 5 (parse 3, search 2),
retrieval eval 1, skills 6, agents + knowledge 5, provider mock 4, fitness 2, model client
1, embedding 1, classification 1, authz 1, policy 1, API 1, runtime 1, demo 1. Full
catalogue with per-test commands: `docs/implementation/TEST-STRATEGY.md`.

### 4.2 Retrieval-quality baseline (fixed set)

| Set | Lexical | Vector | Hybrid |
|---|---|---|---|
| 15 overlapping queries, recall@1 / recall@3 | 1.000 / 1.000 | 1.000 / 1.000 | 1.000 / 1.000 |
| 5 paraphrase queries, recall@3 (recorded, **not asserted**) | 0.600 | 0.600 | 0.800 |

The overlapping-query set is easy by construction; the paraphrase numbers show the real
limit of the non-semantic embedder (E-4, A-29). Asserted regression thresholds: hybrid
recall@3 ≥ 0.9, recall@1 ≥ 0.7, hybrid ≥ lexical.

## 5. Acceptance criteria

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| AC2-01 | Full gate passes; Phase 1 tests green | **Met** | `phase-02-ci.log`; 140 Phase 1 tests ok (one robustness change, §2.4) |
| AC2-02 | Fitness rules for new crates | **Met** | `fitness.rs` (10): layering, `http_client_and_provider_are_confined`, `governed_context_constructors_have_single_producers`, credentials |
| AC2-03 | Sources; versioned, chunked, classified ingestion (MD, text, JSON/OpenAPI) | **Met** | `parse::tests::*`, `knowledge.rs::reference_documents_…`, `::versions_supersede_…` |
| AC2-04 | Quarantine; never retrievable | **Met** | `knowledge.rs::documents_with_credentials_are_quarantined_and_never_retrievable` |
| AC2-05 | Supersede, retire, delete (purge, audit retained) | **Met** | `knowledge.rs::versions_supersede_and_retire_and_delete_purges` |
| AC2-06 | Standards need hash-bound human approval; SoD | **Met** | `knowledge.rs::standards_activate_…`, `::rejected_standards_never_activate`, `authz::tests::knowledge_steward_cannot_approve_…`, `api.rs::knowledge_and_skill_routes_are_governed`, demo log (403/200) |
| AC2-07 | Lexical/vector/hybrid cited results; FTS injection neutralised | **Met** | `search::tests::*`, `embedding::tests::*`, `knowledge.rs::fts_injection_cannot_break_queries_or_scope` |
| AC2-08 | No cross-tenant/project/scope leakage (API, tool, context) | **Met** | `knowledge.rs::scope_tenant_and_classification_isolation`, `::agent_retrieval_is_project_bound_and_ceiling_limited`, `api.rs::knowledge_and_skill_routes_are_governed` |
| AC2-09 | Classification ceilings; restricted never egresses | **Met** | `policy::tests::restricted_content_never_egresses_…`, `model_client.rs::restricted_content_is_not_sent_…`, `knowledge_agents.rs::restricted_knowledge_is_never_sent_…` |
| AC2-10 | Mandatory standards present at `enterprise_policy` trust, not overridable | **Met** | `knowledge_agents.rs::agent_applies_standards_…`, `::injected_knowledge_cannot_override_…`, `knowledge.rs::conflicts_freshness_and_standard_precedence` |
| AC2-11 | Freshness; conflicts with precedence | **Met (subject-key conflicts only)** | `knowledge.rs::conflicts_freshness_and_standard_precedence`; semantic contradictions not detected (KS-L5) |
| AC2-12 | Skill versions, dependencies, cycles, contracts | **Met** | `skills.rs::registration_validation`, `::dependency_cycles_are_rejected`, `::selection_is_deterministic_…` |
| AC2-13 | Approved-only; no permission extension | **Met** | `skills.rs::skills_require_human_approval_bound_to_digest`, `::rejected_skills_never_become_usable`, `::skills_never_extend_agent_permissions` |
| AC2-14 | Deterministic selection; refs in results, usage, provenance | **Met** | `skills.rs::selection_is_deterministic_…`, `knowledge_demo.rs` (provenance + `model_usage`) |
| AC2-15 | Fabricated citations fail | **Met (provenance check)** | `knowledge_agents.rs::fabricated_citations_fail_validation`; claim support not checked (A-36) |
| AC2-16 | Adapter wire format, tool round trip, usage, refusal/error mapping, retry — mock | **Met against mock only** | `mock_api.rs` (4) |
| AC2-17 | Credentials never exposed; provider opt-in | **Met** | `harness.rs::anthropic_provider_is_opt_in_and_needs_a_secret`, `mock_api.rs::error_mapping_and_no_credential_leakage`, `fitness.rs::no_credentials_…`; demo log 0 literals |
| AC2-18 | Knowledge hooks and events; audit | **Met** | `knowledge.rs::knowledge_hooks_observe_and_can_veto_retrieval`, `::reference_documents_…` (audit + events) |
| AC2-19 | API/CLI authenticated, authorized, paginated | **Met** | `api.rs::knowledge_and_skill_routes_are_governed`; CLI exercised by `demo-knowledge.sh` |
| AC2-20 | Demo: agent + approved skill + knowledge → cited, approved artifact | **Met with scripted model**; real-server demo stops at the agent step (no provider) | `knowledge_demo.rs`; `phase-02-knowledge-demo.log` |
| AC2-21 | Retrieval baseline measured and recorded | **Met** | `phase-02-retrieval-eval.log` |
| AC2-22 | Docs, ADRs, roadmap, action register, test strategy, report | **Met** | this report; ADR-0012…0017; `ROADMAP.md`, `ACTION-ITEMS.md`, `TEST-STRATEGY.md`, architecture docs, `OPERATIONS.md`, `README.md` |
| AC2-23 | Live Anthropic API smoke run | **NOT IMPLEMENTED — not authorised (P2-D6)** | — (A-30) |

## 6. Exceptions requiring your decision

| ID | Exception | Risk | Proposed handling |
|---|---|---|---|
| E-1 (Phase 1) | RUSTSEC-2026-0459 (bundled SQLite < 3.54) ignored in `deny.toml`; sqlx latest is still 0.9.0 | Medium; more relevant now that ingested content is stored | Accept until upstream release (A-10); decision requested (P2-D7 / A-34) |
| E-2 (Phase 1) | Container isolation verified only with Podman on macOS | Medium | Accept; verify on Linux/Docker (A-26); decision requested (P2-D7 / A-34) |
| E-3 (new) | Anthropic adapter verified only against a mock server; no live call made | Medium: real API behaviour may differ from the mock (e.g. undocumented fields) | Accept; provider stays disabled by default; perform A-30 before relying on agents in Phase 3 |
| E-4 (new) | Built-in embedder is non-semantic (paraphrase hybrid recall@3 0.8 on 5 queries) | Medium for retrieval quality, no security impact | Accept; A-29 |

## 7. Known limitations

* KS-L1…KS-L7 in `docs/architecture/knowledge-and-skills.md`: non-semantic embedder;
  brute-force vector scan; no YAML/PDF/DOCX/connectors/URL fetch; team knowledge not visible
  to agents; subject-key conflict detection only; citation check is provenance-only;
  adapter mock-verified only.
* Egress protection depends on correct classification at ingestion; task input defaults to
  `internal`.
* Quarantine detects only pattern-recognisable secrets.
* Retrieval, ingestion and the provider were not load-tested.
* Phase 1 carry-overs not addressed in Phase 2 (retargeted to Phase 3 in the action
  register): A-08, A-16, A-18, A-20, A-21 (prompt/policy/agent/workflow admin routes), A-22,
  A-23, A-26, A-28.

## 8. Phase 3 dependencies

| Dependency | Why Phase 3 needs it | Action |
|---|---|---|
| Phase 2 exit decision + Phase 3 authorization | Gate | — |
| Decision on E-1…E-4 | Phase exit | A-34 |
| Live provider verification | BRD analysis and HLD/LLD generation cannot be evaluated with a scripted double | A-30 |
| Data-protection review of egress | Requirements/architecture documents may be confidential | A-33 |
| Real enterprise standards and reference architectures ingested and approved | Generated architecture must cite approved standards | A-09 |
| Semantic retrieval | Requirements text paraphrases standards; lexical retrieval is insufficient | A-29 |
| Admin routes for prompts, agent and workflow definitions | Phase 3 adds many agents/workflows | A-21 |
| Supported ingestion formats for BRDs (e.g. DOCX/PDF) | BRD inputs | A-35 |

Phase 2 components Phase 3 builds on: `KnowledgeService` (standards, references, ADRs as
cited context), `SkillRegistry` (e.g. requirements-analysis and architecture-review skills
under approval), RAG and citation validation in `AgentRuntime`, approval gates for generated
artifacts, egress policy and provider abstraction.

## 9. Exit decision requested

Proposed: **READY WITH ACCEPTED EXCEPTIONS (E-1, E-2, E-3, E-4)**. Please:

1. Accept or reject E-1 and E-2 (P2-D7) and E-3, E-4.
2. Decide whether to authorise a live API verification (A-30) now or at the start of Phase 3.
3. Authorise (or not) committing the Phase 1 and Phase 2 work (A-27).
4. Explicitly authorise Phase 3 when ready. Phase 3 has not been started.
