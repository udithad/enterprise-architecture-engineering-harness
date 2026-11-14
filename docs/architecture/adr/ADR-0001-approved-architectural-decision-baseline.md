# ADR-0001: Approved architectural decision baseline

| Field | Value |
|---|---|
| Status | Accepted |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
`claude-instructions.md` §3 fixes six architectural decisions. Phase 1 must preserve them; any change requires reasoning, alternatives, impact analysis and approval.

## Decision
| Dimension | Decision | Phase 1 realisation |
|---|---|---|
| Orchestration | Hybrid | `eaeh-workflow` deterministic engine (authoritative) + `Planner`/`TaskExecutor` traits implemented in `eaeh-runtime` with agents (advisory) |
| Agent instantiation | Dynamic | `AgentRuntime::instantiate`/`terminate` per task; no long-running agents |
| Execution environment | Isolated | `ExecutionProvider` abstraction; restricted-process and OCI container providers with declared capabilities |
| Software development | Multi-agent | Delegation, typed messages and limits (foundation only; development workflows are Phase 4) |
| Human approvals | Gated | `ApprovalManager` + workflow `approval_gate` states + `requires_approval_of` runtime check + API |
| Core implementation | Rust-first | Cargo workspace, Rust 1.99.1 |

## Consequences
No deviation from the approved decisions was introduced in Phase 1. ADR-0002…0011 refine *how* they are realised.

## References
`docs/implementation/stages/PHASE-01-ASSESSMENT.md` §2, §17.2
