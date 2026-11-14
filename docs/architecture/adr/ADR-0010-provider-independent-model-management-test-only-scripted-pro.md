# ADR-0010: Provider-independent model management; test-only scripted provider; optional OpenTelemetry

| Field | Value |
|---|---|
| Status | Accepted (D-04, D-05) |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
Core orchestration must not depend on one LLM provider. No production provider integration is required in Phase 1. Structured logging and correlation ids are mandatory; OTel export is optional.

## Decision
* `ModelProvider` trait + `ModelRegistry` (routing: preferred first, then lowest unit cost satisfying capabilities/context/quality/residency/cost ceiling; per-provider circuit breaker) + governed `ModelClient` (budgets, LLM hooks, usage/model-version/cost/prompt/context-snapshot recording, `ModelInvoked` events, response redaction; output returned as `Untrusted`).
* The production binary registers **no** model provider; agentic workflows fail with `unavailable` until a provider is registered by the composition root (A-11).
* `ScriptedModelProvider` lives in `eaeh-testkit` (dev-only, fitness-enforced) and is documented as a test double.
* Logging: `tracing` JSON through a redacting writer; correlation/causation ids on events, audit records and model usage. OpenTelemetry export is not implemented (optional per D-05; A-13).

## Consequences
Agent behaviour is verified with deterministic scripts, not real models (risk R-05).
