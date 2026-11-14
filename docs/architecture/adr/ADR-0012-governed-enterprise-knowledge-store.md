# ADR-0012: Governed enterprise knowledge store

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-12 |
| Phase | 2 – Enterprise Knowledge and Skills Management |
| Deciders | Product owner (approval of PHASE-02-PLAN), Principal architect |

## Context
Master §17 requires governed knowledge: source registration, ingestion, classification, versioning, validation, indexing, provenance, access control, lifecycle, freshness and conflict detection, without cross-project/tenant leakage and without letting project content override mandatory standards.

## Decision
* New crate `eaeh-knowledge` (depends on core, store, events, governance, extensibility, validation, intelligence, tools — fitness-enforced).
* **Sources** (tenant-scoped, kind `standard | reference | api_spec | adr | process`) and **immutable document versions** per (source, subject). Formats: Markdown (heading-aware chunks), plain text, JSON and OpenAPI-JSON (one chunk per operation/schema) — P2-D3. No URL fetching.
* **Validation gate**: the deterministic `secret.leak` validator runs on ingestion; failing documents are **quarantined** — content is *not stored*, nothing is indexed, metadata/hash/findings are kept and audited.
* **Lifecycle**: `quarantined | pending_approval → active → superseded | retired → deleted`; delete purges content, chunk text, FTS entries and vectors, keeps a tombstone and the audit trail.
* Documents of `standard` sources require **human approval bound to the content hash** (ADR-0017) before activation.
* **Conflicts**: on activation, active documents with the same subject from other sources and different content are recorded with a precedence resolution (mandatory first, then scope enterprise > domain > project > team > workflow).
* Storage: migration `0002` (`knowledge_sources`, `knowledge_documents`, `knowledge_chunks`, FTS5 `knowledge_chunks_fts`, `knowledge_embeddings`, `knowledge_conflicts`); all writes through the serialized writer.
* Citations are deterministic and human-readable: `source/subject@vN#chunk` (document-level `source/subject@vN`).

## Consequences
Knowledge is auditable and reproducible. SQLite brute-force vector search bounds corpus size (R2-04); the `EmbeddingProvider` and storage seams allow a vector store later.

## Alternatives considered
External vector database (rejected for Phase 2: new infrastructure); URL/connector ingestion (deferred: SSRF and credential surface).
