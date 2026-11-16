# Architecture Decision Records

| ADR | Title | Status |
|---|---|---|
| [ADR-0001](ADR-0001-approved-architectural-decision-baseline.md) | Approved architectural decision baseline | Accepted |
| [ADR-0002](ADR-0002-modular-monolith-with-fitness-enforced-crate-layering.md) | Modular monolith with fitness-enforced crate layering | Accepted |
| [ADR-0003](ADR-0003-third-party-dependency-selection-and-supply-chain-gate.md) | Third-party dependency selection and supply-chain gate | Accepted (one exception pending approval) |
| [ADR-0004](ADR-0004-persistence-sqlite-via-sqlx-with-serialized-immediate-write.md) | Persistence: SQLite via SQLx with serialized IMMEDIATE write transactions | Accepted (D-01) |
| [ADR-0005](ADR-0005-distinct-hooks-middleware-interceptors-and-a-transactional-e.md) | Distinct hooks, middleware, interceptors and a transactional event outbox | Accepted |
| [ADR-0006](ADR-0006-in-house-deterministic-policy-engine.md) | In-house deterministic policy engine | Accepted (D-02) |
| [ADR-0007](ADR-0007-toolchain-pin-rust-1-99-1.md) | Toolchain pin: Rust 1.99.1 | Accepted (D-06) |
| [ADR-0008](ADR-0008-plugin-model-trusted-in-process-extensions-only.md) | Plugin model: trusted in-process extensions only | Accepted (D-03, clarification 1) |
| [ADR-0009](ADR-0009-isolation-providers-declare-genuinely-enforced-capabilities.md) | Isolation providers declare genuinely enforced capabilities; OCI container provider | Accepted (D-07, clarifications 2–3) |
| [ADR-0010](ADR-0010-provider-independent-model-management-test-only-scripted-pro.md) | Provider-independent model management; test-only scripted provider; optional OpenTelemetry | Accepted (D-04, D-05) |
| [ADR-0011](ADR-0011-credential-provisioning-token-rotation-and-revocation.md) | Credential provisioning, token rotation and revocation | Accepted (clarification 4) |
| [ADR-0012](ADR-0012-governed-enterprise-knowledge-store.md) | Governed enterprise knowledge store | Accepted |
| [ADR-0013](ADR-0013-hybrid-retrieval-with-a-deterministic-local-embedder.md) | Hybrid retrieval with a deterministic local embedder | Accepted (P2-D2) |
| [ADR-0014](ADR-0014-skills-as-governed-permission-preserving-capabilities.md) | Skills as governed, permission-preserving capabilities | Accepted |
| [ADR-0015](ADR-0015-anthropic-messages-api-adapter-credentials-and-egress-govern.md) | Anthropic Messages API adapter, credentials and egress governance | Accepted (P2-D1, P2-D5) |
| [ADR-0016](ADR-0016-knowledge-scopes-abac-attributes-and-classification-ceilings.md) | Knowledge scopes, ABAC attributes and classification ceilings | Accepted |
| [ADR-0017](ADR-0017-knowledge-steward-role-and-approval-separation-of-duties.md) | knowledge_steward role and approval separation of duties | Accepted (P2-D4) |
| [ADR-0018](ADR-0018-typed-requirements-model-with-deterministic-analysis.md) | Typed requirements model with deterministic analysis | Accepted (P3-D2, P3-D5) |
| [ADR-0019](ADR-0019-typed-hld-lld-models-with-deterministic-rendering.md) | Typed HLD/LLD models with deterministic rendering | Accepted (P3-D3) |
| [ADR-0020](ADR-0020-request-changes-decisions-and-bounded-revisions-on-approval.md) | Request-changes decisions and bounded revisions on approval gates | Accepted (P3-D4) |
| [ADR-0021](ADR-0021-quality-gates-input-bindings-and-bundles-in-workflows-valida.md) | Quality gates, input bindings and bundles in workflows; validators in the correction loop | Accepted |
| [ADR-0022](ADR-0022-traceability-store-computed-from-approved-artifacts.md) | Traceability store computed from approved artifacts | Accepted |
| [ADR-0023](ADR-0023-deterministic-replay-model-provider.md) | Deterministic replay model provider | Accepted (amends ADR-0010; requested as 'deterministic mode') |
| [ADR-0024](ADR-0024-developer-experience-identity-profiles-project-manifest-read.md) | Developer experience: identity profiles, project manifest, readiness, activities and MCP integration | Accepted |
| [ADR-0025](ADR-0025-external-ai-execution-mode.md) | External AI execution mode (Claude Code / Copilot reasoning via MCP hand-offs) and governed software delivery | Accepted (amends ADR-0010, ADR-0023, ADR-0024) |
| [ADR-0026](ADR-0026-governance-portal-local-access-model.md) | Governance Portal and its local access model (no login, injected stored identities, launch secret, loopback only) | Accepted (supplementary UI enhancement, Phase 1–3 scope) |
| [ADR-0027](ADR-0027-approved-artifact-materialization.md) | Approved design documents materialized into the project workspace (engine-driven on approval, hash-verified, blocks on failure) | Accepted (supplementary fix, Phase 1–3 scope) |
| [ADR-0028](ADR-0028-markdown-skills-and-portal-skill-governance.md) | Markdown skills (TOML front matter), layer skills by task kind, skill registration and decisions in the portal | Accepted (amends ADR-0026 §4; Phase 1–3 scope) |
| [ADR-0029](ADR-0029-skill-directories-and-automatic-discovery.md) | Skill directories (SKILL.md + nested files), automatic discovery, tree-hash-bound versions, reference files in agent context | Accepted (extends ADR-0028; Phase 1–3 scope) |
| [ADR-0030](ADR-0030-community-skill-md-yaml-front-matter.md) | Community SKILL.md with YAML front matter (name, description, license, metadata; extra properties preserved; layers explicit or inferred from metadata.scope; yaml-rust2) | Accepted (supersedes the TOML-only part of ADR-0028/0029; Phase 1–3 scope) |
