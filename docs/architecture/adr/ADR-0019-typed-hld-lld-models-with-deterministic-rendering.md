# ADR-0019: Typed HLD/LLD models with deterministic rendering

| Field | Value |
|---|---|
| Status | Accepted (P3-D3) |
| Date | 2026-11-12 |
| Phase | 3 – Requirements Engineering and Architecture Generation |
| Deciders | Product owner (approval of PHASE-03-PLAN), Principal architect |

## Context
Master §30 requires HLD/LLD generation with architecture, component, API, data, sequence and deployment design, diagrams, ADRs, standards compliance, risk and trade-off analysis. Diagrams drawn by a model can disagree with the design and can carry injected content.

## Decision
* New crate `eaeh-architecture` (depends on core, validation, requirements only; fitness-enforced).
* Typed `Hld` (system context with actors, components with kinds and allocated requirements, interfaces, data flows, deployment nodes, decisions, risks, trade-offs, compliance entries, citations) and `Lld` (component designs refining HLD ids, API operations, data entities, sequences, deployment, LLD decisions, risks, compliance).
* Deterministic validators `architecture.hld` / `architecture.lld`: referential integrity, slug ids, every Must requirement allocated (HLD) and realised (LLD), LLD refines the approved HLD (no invented components; every buildable HLD component designed), API/entity/sequence consistency, decisions with alternatives (trade-off analysis), risks with mitigations, and **compliance with every mandatory standard supplied to the architect** (deviation ⇒ justification + decision/ADR).
* **Rendering is deterministic and owned by the platform:** Markdown documents, ADRs (one per decision) and Mermaid diagrams (context, component, deployment, sequence, ER, component internals) are generated from the validated model; identical models give byte-identical bundles. Model text is entity-encoded and identifiers reduced to `[A-Za-z0-9_]`, so content cannot inject Mermaid directives, click handlers or raw HTML.
* Trace links (requirement → component/interface/decision, decision → standard, HLD component → LLD design, requirement → API/sequence) are computed from the models.

## Consequences
Diagrams always match the approved model; no image-rendering infrastructure (Mermaid text renders in common Markdown viewers). Quality of the design content itself still depends on the model and human review.

## Alternatives considered
PlantUML (rejected: Java tooling); model-authored diagrams (rejected: inconsistency and injection risk); image rendering (deferred).
