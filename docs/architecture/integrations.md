# External Integrations

Phase 1 deliberately contains **no** domain integrations (master §28). The adapter pattern
that later integrations must follow is established:

| Integration | Phase | Adapter seam already present |
|---|---|---|
| LLM providers (`direct_provider` mode) | **2 – Anthropic Messages API adapter implemented** (`eaeh-provider-anthropic`, opt-in, mock-verified; not registered in `external_ai` mode) | `ModelProvider` trait; further providers via `HarnessBuilder::model_provider` or a plugin with `register_model_providers` |
| Execution backends (microVM, Kubernetes jobs) | 8 | `ExecutionProvider` trait with declared capabilities |
| Secrets backends (Vault, cloud KMS) | 2–8 | `SecretsProvider` trait |
| Enterprise identity (OIDC/SAML) | 8 | `IdentityService::authenticate` boundary in the API middleware |
| Knowledge sources | 2 – inline/CLI-directory ingestion implemented; connectors deferred | `KnowledgeService::ingest`; `EmbeddingProvider` for semantic embeddings later |
| Git providers, CI/CD | 6 | Tools via `ToolRegistry` (risk class `git`), engineering hooks (commit/PR/build/test) defined |
| Deployment / cloud infra | 7 | Risk classes `cloud_infra`/`deployment` already require human approval in the baseline policy |
| AI clients as the reasoning engine (`external_ai` mode) | **Implemented** (ADR-0025): Claude Code and GitHub Copilot (VS Code agent mode) perform agent reasoning under the user's own subscription through task hand-offs. EAEH makes no model calls in this mode. Verified: a real Claude Code 2.1.323 session over MCP (read-only tools) and protocol-level tests for both clients. A real Copilot session has not been run. | `eaeh-agents::external`, `external_tasks` table |
| MCP (EAEH as server for AI clients) | **Implemented** (ADR-0024, ADR-0025): `eaeh mcp` stdio server with tools for context, readiness, knowledge, activities, status, approvals (read-only), artifacts and trace, plus the hand-off tools `eaeh_task_list/get/tool/submit`. No approval tool. Provider-neutral: one server and one instruction text. | `crates/eaeh-cli/src/mcp.rs`; generated `.mcp.json` (Claude Code) and `.vscode/mcp.json` (Copilot) |
| MCP servers as tool sources | when needed | Would register tools in `ToolRegistry`; the core does not depend on MCP |
| OpenTelemetry | optional (D-05) | `tracing` spans/fields; exporter not wired |

All adapters remain subject to the tool gateway, policy engine, approvals, budgets and audit.
