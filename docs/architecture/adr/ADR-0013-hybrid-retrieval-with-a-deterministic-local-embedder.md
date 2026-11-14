# ADR-0013: Hybrid retrieval with a deterministic local embedder

| Field | Value |
|---|---|
| Status | Accepted (P2-D2) |
| Date | 2026-11-12 |
| Phase | 2 – Enterprise Knowledge and Skills Management |
| Deciders | Product owner (approval of PHASE-02-PLAN), Principal architect |

## Context
Master §17 asks for hybrid/vector retrieval where appropriate and reranking where beneficial. No embedding endpoint of the chosen model provider is assumed, and external embedding calls would add egress.

## Decision
* Retrieval modes: **lexical** (FTS5 BM25), **vector** (cosine over stored vectors) and **hybrid** (reciprocal rank fusion, k = 60; default).
* `EmbeddingProvider` trait; built-in `HashedNgramEmbedder` (word uni/bigrams + character trigrams, feature hashing, L2-normalised, 384 dims by default) — local, deterministic, **not semantic**.
* Deterministic reranking: relevance dominates; a heading match is a 1 % nudge; stale items are halved; source authority (standard > adr > spec/process > reference) and the mandatory flag only break ties. (An earlier multiplicative authority boost let an unrelated standard outrank the relevant guide; found by the demo test and corrected.)
* Every query is filtered by tenant, status, scope visibility and classification ceiling *in SQL*, and audited (`knowledge.search`: query hash, citations).
* User query text is reduced to quoted alphanumeric terms before `MATCH`, so FTS5 syntax cannot be injected.

## Consequences
Measured baseline (fixed set, `retrieval_eval.rs`): recall@3 = 1.0 for all modes on 15 lexically-overlapping queries; on 5 pure paraphrases recall@3 = 0.6 lexical, 0.6 vector, **0.8 hybrid**. Semantic quality requires a semantic embedding provider (A-29).

## Alternatives considered
External embedding API (deferred decision); local neural model via ONNX (heavy native dependencies, model distribution).
