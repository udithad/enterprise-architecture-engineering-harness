# ADR-0023: Deterministic replay model provider

| Field | Value |
|---|---|
| Status | Accepted (amends ADR-0010; requested as 'deterministic mode') |
| Date | 2026-11-13 |
| Phase | Developer-experience improvement (within the Phase 1–3 scope) |
| Deciders | Product owner (developer-experience request), Principal architect |

## Context
ADR-0010 kept the only deterministic model double (`ScriptedModelProvider`) test-only. The shipped binary therefore could not run any agent workflow without paid API access, so onboarding and acceptance scenarios could not be exercised end to end. The developer-experience request asks for a deterministic mode so the acceptance scenario does not depend on a paid LLM.

## Decision
* New production `ModelProvider` `eaeh-intelligence::replay::ReplayProvider`, configured by `[providers.replay] enabled, cassette`. It is **disabled by default**.
* A cassette is a JSON list of entries. Each entry's `match` is a string found in the agent's system message (e.g. `'business-analyst' agent`), and its `responses` (tool call, JSON or text) are returned in order. When a cassette runs out or no entry matches, the call fails as a non-retryable error.
* Egress is `local` and cost is zero. The reported model version is `replay-cassette-<sha256 prefix>`, so model-usage records, artifact provenance and audit show that output was **replayed, not AI-generated**. Readiness reports it as a warning.
* Replay responses pass through exactly the same governance as model output: untrusted handling, tool gateway, schema/skill/citation/domain validators, quality gates, approvals, traceability, audit.
* The test double stays test-only (fitness rule unchanged).

## Consequences
Workflows can be demonstrated and accepted offline and reproducibly. Replayed fixtures prove the *pipeline*, never generation quality, and must not be presented as AI output. Cassettes are tied to prompts and BRD citations, so they must be re-recorded when those change.

## Alternatives considered
Promote the test double to production (rejected: test API, no provenance labelling); a local open-weights model (rejected: new infrastructure, still non-deterministic).
