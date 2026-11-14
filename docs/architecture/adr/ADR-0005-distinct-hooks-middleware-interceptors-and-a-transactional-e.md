# ADR-0005: Distinct hooks, middleware, interceptors and a transactional event outbox

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
The master instructions require hooks, middleware, interceptors and events with distinct responsibilities, and forbid mandatory controls depending solely on hooks.

## Decision
* **Hooks** (`HookRegistry`): 42 typed lifecycle points; ordering by priority then id; subject/tenant filters; per-hook timeouts; block / warn / continue failure policies; vetoes honoured only at `Before*` points (they can only restrict).
* **Middleware** (`Pipeline<Req, Res>`): ordered onion around an endpoint; may short-circuit; errors propagate unchanged; pipeline timeout.
* **Interceptors** (`InterceptorChain<T>`): synchronous transformation of a value crossing a boundary (secret redaction, size limits on tool output).
* **Events**: typed, versioned envelopes written to an outbox in the same transaction as the state change; at-least-once dispatcher with per-consumer delivery state, exponential backoff with jitter, dead-letter + audit, optional per-consumer ordering. No broker.
* Mandatory controls (authentication, authorization, autonomy, input contracts, policy, approvals, budgets, isolation selection) run in trusted code **before** any hook or middleware.

## Consequences
Clear extension contracts. At-least-once delivery requires consumers with external side effects to use idempotency keys (`Db::record_idempotent_in`).

## Alternatives considered
External message broker (rejected for Phase 1, master §8).
