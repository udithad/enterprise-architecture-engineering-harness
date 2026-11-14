# ADR-0015: Anthropic Messages API adapter, credentials and egress governance

| Field | Value |
|---|---|
| Status | Accepted (P2-D1, P2-D5) |
| Date | 2026-11-12 |
| Phase | 2 – Enterprise Knowledge and Skills Management |
| Deciders | Product owner (approval of PHASE-02-PLAN), Principal architect |

## Context
Phase 2 needs a production LLM provider without hard-coded credentials or bypassing security controls. Rust has no official Anthropic SDK.

## Decision
* New crate `eaeh-provider-anthropic` (deps: core, intelligence; the **only** crate allowed to depend on an HTTP client — fitness-enforced; only `eaeh-runtime` may depend on it).
* Raw HTTPS `POST /v1/messages`; headers `x-api-key`, `anthropic-version: 2026-10-21`; tools `{name, description, input_schema}` with reversible name encoding (`knowledge.search` ↔ `knowledge_search`); `tool_choice` omitted (auto); **`temperature`/`top_p`/`top_k` never sent**; optional `output_config.effort`; server-side refusal fallback (`fallbacks: "default"`, beta `server-side-fallback-2026-07-01`) on by default.
* Mapping: 429/5xx/529/transport → `unavailable` (retried with `retry-after`, then eligible for `ModelClient` fallback); 408 → `timeout`; 401/403 → `forbidden`; other 4xx → `invalid_input`; `stop_reason: "refusal"` → `policy_denied`.
* Credentials: `EAEH_SECRET_ANTHROPIC_API_KEY` via `SecretsProvider` only; registered with the redactor; never in config (fitness), logs, audit, `Debug` or errors. The provider is **disabled unless** `providers.anthropic.enabled = true`; enabled without a key fails startup with an explicit message. Base URL must be HTTPS (plain HTTP only for localhost testing).
* Default catalogue: `claude-opus-5-5` (1 M context, 128 K output, $4/$20 per MTok). Routing still picks the cheapest model meeting an agent's requirements among configured models.
* TLS: `reqwest` 0.13 with `rustls-no-provider` + `rustls` `ring` provider (avoids aws-lc's C toolchain). `cargo-deny` licence allow-list adds `CDLA-Permissive-2.0` (Mozilla CA certificate data in `webpki-root-certs`).
* **Egress governance**: `ModelProvider::egress()` (`external` by default, fail-safe). Before every model call `ModelClient` evaluates policy action `model.invoke` with attributes `egress` and `classification` (highest classification in the assembled context). Baseline: allow, except **deny `restricted` → `external`**. Denials are audited; if every candidate is denied the call fails with `policy_denied` and nothing is sent.

## Consequences
The adapter is verified against a local mock server only; no live API call has been made (P2-D6 not authorised). Live verification is action A-30.
