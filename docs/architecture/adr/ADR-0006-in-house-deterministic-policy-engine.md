# ADR-0006: In-house deterministic policy engine

| Field | Value |
|---|---|
| Status | Accepted (D-02) |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
Policies must be deterministic, independently enforceable, versioned, inheritable and auditable; an LLM may never authorize.

## Decision
`PolicyEngine` trait with `RuleEngine`: JSON policy documents (enterprise or project level); conditions over action, risk class, principal kind, autonomy, resource id and attributes; effects allow / deny / require_approval(role). Default deny; deny overrides require_approval overrides allow (so a project policy can never weaken an enterprise rule); matched rules are returned and audited. Only the structural `eaeh.baseline` policy ships — **no enterprise standards are invented** (A-09).

## Consequences
Simple, fast, fully deterministic and tested. A Cedar adapter can implement the same trait later.

## Alternatives considered
Cedar (deferred: heavier dependency; can be added behind the trait).
