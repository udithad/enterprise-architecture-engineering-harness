# EAEH Action Register

Updated: 2026-11-13 (developer-experience improvements; Phase 3 exit decision pending). Priority: Critical / High / Medium / Low.

| ID | Description | Priority | Dependency | Responsible role | Target phase | Status |
|---|---|---|---|---|---|---|
| A-01 | Approve Phase 1 architecture proposal and scope | Critical | – | Product owner | 1 | **Done** (approved 2026-11-12) |
| A-02 | Resolve decisions D-01…D-07 | High | A-01 | Product owner / Architect | 1 | **Done** (recorded in ADR-0001…0011) |
| A-03 | Provide a running container runtime | High | – | Platform engineer | 1 | **Done** (Podman 5.5.1 machine running) |
| A-04 | ADRs for approved decisions and clarifications | High | A-02 | Architect | 1 | **Done** (`docs/architecture/adr/`) |
| A-05 | Architecture docs, ROADMAP, ACTION-ITEMS, TEST-STRATEGY, THREAT-MODEL | High | A-01 | Engineering lead | 1 | **Done** |
| A-06 | README with purpose, build and usage | Medium | – | Engineering lead | 1 | **Done** |
| A-07 | Install `cargo-deny` and add supply-chain gate | Medium | – | Platform engineer | 1 | **Done** (0.20.2, `deny.toml`, `scripts/ci.sh`) |
| A-08 | Decide CI hosting (e.g. GitHub Actions) and run `scripts/ci.sh` + container suites there (Linux) | Medium | – | Platform engineer | 4 | Open (not addressed in Phases 2–3) |
| A-09 | Supply enterprise policies/standards and reference architectures (EAEH must not invent them) | High | – | Enterprise architecture | 4 | Open — required before Phase 3 designs are relied on; only labelled demonstration content is loaded |
| A-10 | Upgrade to sqlx with `libsqlite3-sys >= 0.39` when released and remove the RUSTSEC-2026-0459 ignore | High | sqlx release | Engineering lead | 4 | Open — exception E-1 accepted 2026-11-12; upgrade still blocked (sqlx 0.9.0 is latest as of the Phase 3 plan) |
| A-11 | Select and implement the first production `ModelProvider` adapter (provider choice, data-residency/privacy review, secrets via `SecretsProvider`) | High | Provider decision | Architect / Security | 2 | **Done (implementation)** — `eaeh-provider-anthropic` (P2-D1, ADR-0015), opt-in, mock-verified; live verification is A-30; data-protection review before enabling is A-33 |
| A-12 | WASM and external-process plugin execution hosts (required before any untrusted third-party plugin) | Medium | – | Architect | 8 (or earlier on demand) | Open |
| A-13 | Optional OpenTelemetry exporter (D-05) | Low | – | Platform engineer | 2+ | Open |
| A-14 | Enterprise identity integration (OIDC/SAML), token binding/MFA for admin operations | Medium | IdP decision | Security architect | 8 | Open |
| A-15 | Encryption at rest, vault/KMS `SecretsProvider`, data classification, retention & deletion | Medium | – | Security architect | 4–8 | **Partial** — classification + egress rule, knowledge purge (Phase 2), confidential-by-default BRDs (Phase 3); encryption at rest, vault/KMS, retention open |
| A-16 | SBOM generation in CI (e.g. CycloneDX) | Medium | A-08 | Platform engineer | 4 | Open (not addressed in Phases 2–3) |
| A-17 | Shared/distributed rate limiting and pre-authentication flood protection | Low | – | Platform engineer | 8 | Open |
| A-18 | External anchoring of the audit chain head (periodic signed export) to detect whole-chain rewrites | Medium | – | Security architect | 4 | Open (not addressed in Phases 2–3) |
| A-19 | Automated backup/restore and disaster recovery procedures | Medium | – | Operations | 8 | Open |
| A-20 | Stress test multi-process writers (CLI + server on one database) | Medium | – | Engineering lead | 4 | Open (not addressed in Phases 2–3) |
| A-21 | HTTP/CLI admin routes for prompts, policies, agent and workflow definitions (TD-05) | Medium | – | Engineering lead | 4 | Open — Phases 2–3 added knowledge, skill, BRD, trace and export commands only; Phase 3 agents/workflows are file-loaded |
| A-22 | Emit `task.assigned`, `policy.evaluated`, `budget.exhausted` events (TD-04) | Low | – | Engineering lead | 4 | Open (Phases 2–3 added only knowledge/skill/approval/trace events) |
| A-23 | Enforce `wall_clock_ms` and `delegation_depth` through the budget ledger (TD-03) | Low | – | Engineering lead | 4 | Open (not addressed in Phases 2–3) |
| A-24 | Storage abstraction / PostgreSQL adapter (TD-01) | Low | Scale requirement | Architect | 8 | Open |
| A-25 | Task heartbeats during long-running executions (TD-06) | Medium | – | Engineering lead | 4 | Open |
| A-26 | Verify isolation suites on Linux hosts and with Docker (ISO-L2) | Medium | A-08 | Platform engineer | 4 | Open (exception E-2 accepted; not addressed in Phases 2–3) |
| A-27 | Commit the Phase 1–3 work to version control (nothing has been committed by the implementer) | High | Authorisation | Product owner | 1–3 | Open |
| A-28 | Expose plugin registration for middleware, interceptors and validators in `ExtensionContext` (TD-08) | Low | – | Engineering lead | 4 | Open (not addressed in Phases 2–3) |
| A-29 | Select a semantic `EmbeddingProvider` (local model or approved external service, subject to the egress policy) and re-measure the retrieval baseline (paraphrase hybrid recall@3 currently 0.8) | Medium | Data-protection review if external | Architect | 4 | Open (not addressed in Phase 3) |
| A-30 | Live run of the adapter and of `engineering.design` against the real API (`EAEH_DEMO_LIVE=1 EAEH_ANTHROPIC_KEY_FILE=… scripts/demo-design.sh`) and recording of results/cost | **Critical** (before relying on Phase 3 output) | Operator-supplied key | Product owner / Engineering lead | 3 exit / 4 entry | **Open** — authorised (P3-D1) but not executed: no key was supplied; the product owner chose to skip |
| A-31 | Team-scoped knowledge for agents (derive teams from the workflow initiator) | Low | – | Engineering lead | 4 | Open |
| A-32 | Vector index beyond brute-force scan (ANN / external vector store behind a trait) once corpus size requires it; measure retrieval latency | Low | Scale requirement | Architect | 8 | Open |
| A-33 | Data-protection / residency review before enabling any external model provider (what classifications may egress — BRDs default to `confidential`, which the baseline allows to egress) | High | – | Security architect / DPO | 4 (before production use) | Open |
| A-34 | **Record P2-D7**: explicit acceptance (or rejection) of Phase 1 exceptions E-1 (SQLite advisory) and E-2 (isolation verified only with Podman on macOS) | High | – | Product owner | 2 (exit) | **Done** — E-1…E-4 accepted 2026-11-12 |
| A-35 | Additional ingestion formats and connectors (YAML, PDF, DOCX BRDs, Confluence/Git sources) with SSRF-safe fetching | Medium | P2-D3/P3-D2 follow-up | Engineering lead | 4+ | Open |
| A-36 | Evaluate whether cited text supports agent claims (beyond citation provenance) | Medium | Real-model evaluation | Engineering lead | 5 | Open |
| A-37 | Independent labelled evaluation sets for the requirements analyzer and for generated requirements/HLD/LLD (current labels by the implementer: precision 0.923 / recall 0.857 / pair accuracy 0.875) | Medium | Real-model runs (A-30) | QA lead | 5 | Open |
| A-38 | Phase 3 exit decision (exceptions E-1…E-5, see PHASE-03-COMPLETION) | High | – | Product owner | 3 (exit) | Open |
| A-39 | Dedicated knowledge source kind `brd` (needs a migration that rebuilds `knowledge_sources` and its dependent tables) — Phase 3 uses kind `reference` + tag `brd` (plan deviation) | Low | – | Engineering lead | 4+ | Open |
| A-40 | Streaming responses and per-section generation for large designs (max output tokens per agent: 16k–24k; non-streaming requests) | Medium | A-30 findings | Engineering lead | 4 | Open |
| A-41 | Review UX for bundles (rendered Mermaid/Markdown preview, diff between versions of a revised design) | Low | – | Product owner | 4+ | Open |
| A-42 | Traceability queries: version-aware filtering (latest approved only), indexing beyond the 20,000-link in-memory scan, export (e.g. CSV/RTM) | Low | – | Engineering lead | 4 | Open |
| A-43 | Store CLI credentials in the OS keychain (macOS Keychain, Secret Service, Windows Credential Manager) instead of the 0600 file | Medium | – | Security architect | 4 | Open (developer experience) |
| A-44 | Verify the `eaeh` MCP server with a real Claude Code session (only the protocol was tested automatically) and consider Claude Code hooks that block hand-written governed documents | High | – | Engineering lead | 4 | **Partly closed** (2026-11-13): a real Claude Code 2.1.323 headless session used the EAEH MCP tools (read-only; `evidence/claude-code-mcp-session.log`). Open: a full interactive session doing hand-offs over MCP; hooks against hand-written governed documents |
| A-45 | HTTP API equivalents for `project init`, `project readiness`, `approval review`, activity runs (CLI/MCP only today) | Medium | – | Engineering lead | 4 | Open (developer experience) |
| A-46 | Cassette recording tool (record a live run into a replay cassette, with prompt/BRD hashes to detect staleness) | Low | A-30 | Engineering lead | 4 | Open (developer experience) |
| A-47 | Optional interactive mode for `project init` (prompts for missing values) | Low | – | Engineering lead | 4+ | Open (developer experience) |
| A-48 | Verify GitHub Copilot (VS Code agent mode) with a real session: `.vscode/mcp.json` discovery, tool use, hand-off round trip (only the protocol and generated files are tested) | High | VS Code + Copilot licence | Engineering lead | 4 | Open (external AI) |
| A-49 | Build-verification toolchain image for `engineering.delivery` (JDK 21 + Maven with an offline dependency cache, or a governed network policy for dependency download) and an end-to-end `required` run | High | Container runtime | Platform engineering | 4 | Open (external AI) |
| A-50 | Complete the `simple-springboot-api` external-AI acceptance run through HLD, LLD and delivery. The run waits at the requirements gate because approval gates need a human approver (an AI client must not approve its own work). | High | Human approver | Product owner | – | Open (external AI) |
| A-51 | HTTP API equivalents for `task list/show/tool/submit` (CLI/MCP only today) | Medium | – | Engineering lead | 4 | Open (external AI) |
| A-52 | Client-side enforcement against hand-written governed artifacts, e.g. Claude Code hooks or repository pre-commit checks that require artifacts to come from `eaeh artifact export` of an approved bundle | Medium | – | Security architect | 4 | Open (external AI) |
| A-53 | Hand-off package size: large BRDs/designs make packages big; consider paging `data` and pointing the client to `knowledge.read` instead of inlining | Low | Real usage | Engineering lead | 4 | Open (external AI) |
| A-54 | Data-protection decision for handing confidential BRDs to third-party AI clients (extends A-33 to `external_ai`) | High | – | Security / DPO | – | Open (external AI) |
