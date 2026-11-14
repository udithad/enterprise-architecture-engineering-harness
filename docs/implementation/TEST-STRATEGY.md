# EAEH Test Strategy (Phases 1–3)

All tests are automated, deterministic and repeatable. They are executed by
`scripts/ci.sh` (`cargo test --workspace --locked --no-fail-fast`) after `fmt`, `clippy
-D warnings` and `build`, followed by `cargo deny check`. **Current evidence (Phase 3):**
- `docs/implementation/evidence/phase-03-ci.log`: the full gate output, 214 tests.
- `phase-03-analyzer-eval.log`: the requirements-analyzer baseline.
- `phase-03-design-demo.log`: `scripts/demo-design.sh` against a real server with no model provider.
- `phase-03-api-smoke.log` and `phase-03-knowledge-demo-regression.log`: Phase 1 and Phase 2 real-server regressions.

Phase 1 and Phase 2 evidence (`phase-01-*.log`, `phase-02-*.log`) is retained unchanged.

Never fabricate results: a test counts as passed only when it appears as `ok` in an
evidence log produced by the commands below.

## Test levels

| Level | Where | Count (Phase 3 total) |
|---|---|---|
| Unit | `crates/*/src/**` `#[cfg(test)]` | see catalogue |
| Component / integration | `crates/*/tests/*.rs` | see catalogue |
| API contract | `crates/eaeh-api/tests/api.rs` | 7 |
| CLI end-to-end (real binary) | `crates/eaeh-cli/tests/cli.rs` | 2 |
| Security & governance | spread across governance, tools, agents, workflow, api, e2e | see §36 mapping |
| Isolation (real escape attempts) | `crates/eaeh-isolation/tests/*` | 13 |
| Failure & recovery | workflow engine, events, supervision | see catalogue |
| End-to-end | `tests/tests/e2e.rs`, `tests/tests/knowledge_demo.rs`, `tests/tests/design_demo.rs` | 7 |
| Logging (own process, global subscriber) | `tests/tests/logging.rs` | 1 |
| Architecture fitness | `tests/tests/fitness.rs` | 11 |
| Knowledge & retrieval (Phase 2) | `crates/eaeh-knowledge` (unit + `tests/knowledge.rs`) | 15 |
| Retrieval-quality evaluation (Phase 2) | `crates/eaeh-knowledge/tests/retrieval_eval.rs` | 1 (recall@k baseline) |
| Skills (Phase 2) | `crates/eaeh-skills/tests/skills.rs` | 6 |
| Agent + knowledge/skills integration (Phase 2) | `crates/eaeh-agents/tests/knowledge_agents.rs` | 5 |
| Provider adapter vs. mock HTTP server (Phase 2) | `crates/eaeh-provider-anthropic/tests/mock_api.rs` | 4 |
| Knowledge demo (real server, scripted) | `scripts/demo-knowledge.sh` | 1 script |
| Requirements model & analysis (Phase 3) | `crates/eaeh-requirements` (unit + `tests/requirements.rs`) | 9 |
| Analyzer quality evaluation (Phase 3) | `crates/eaeh-requirements/tests/analyzer_eval.rs` | 1 (precision/recall baseline) |
| HLD/LLD validation, rendering, traceability (Phase 3) | `crates/eaeh-architecture` (unit + `tests/architecture.rs`) | 9 |
| Workflow revisions, bindings, gates, bundles, trace (Phase 3) | `crates/eaeh-workflow/tests/design_flow.rs` | 5 |
| Design tools (Phase 3) | `crates/eaeh-engineering/tests/tools.rs` | 2 |
| Design demo (real server, no model) | `scripts/demo-design.sh` | 1 script |
| API smoke (manual/scripted, real server) | `scripts/smoke-api.sh` | 1 script |
| Performance/load, agent evaluation (real models), live provider API | — | **not performed** (Phase 5; Phase 3 live run authorised (P3-D1) but not executed – no API key supplied) |

**Total automated tests: 214** (140 Phase 1 + 41 Phase 2, all passing, + 33 added in Phase 3).

## Global prerequisites and configuration

* Rust 1.99.1 (`rust-toolchain.toml`), run from the repository root.
* Container suites: Podman or Docker running with `docker.io/library/alpine:3.20` pulled.
  Container tests are **fail-closed**: without a runtime they fail unless
  `EAEH_SKIP_CONTAINER_TESTS=1` is set, which prints `SKIPPED` (never reported as passed).
* Network is not required at test time (`cargo metadata --offline`, `--pull never`).
* Each test creates its own temporary database/data directory; tests are independent and
  run in parallel.

## Exact execution instructions

```bash
scripts/ci.sh                                         # complete gate (expected final line: "==> CI PASSED")
cargo test --workspace --locked --no-fail-fast        # all tests
cargo test -p <crate> [--test <file>] [<name filter>] # a single suite or test
scripts/smoke-api.sh                                  # real-server smoke (expected final line: "== SMOKE PASSED")
scripts/demo-knowledge.sh                             # knowledge demo (expected final line: "== DEMO COMPLETE (live model: false)")
scripts/demo-design.sh                                # design demo (expected final line: "== DEMO COMPLETE (live model: false)")
cargo test -p eaeh-requirements --test analyzer_eval -- --nocapture # prints ANALYZER-EVAL lines
cargo test -p eaeh-knowledge --test retrieval_eval -- --nocapture   # prints RETRIEVAL-EVAL lines
```

Pass/fail criterion for every automated test: the test function returns without panic
(`test <name> ... ok`); any failed assertion is a failure. Expected behaviour for each test
is expressed by its assertions in the referenced source file; sample data is constructed in
the test (no external fixtures).

## Mandatory scenarios (master §36) → tests

| Scenario | Tests (file::name) | Result in evidence |
|---|---|---|
| Agent cannot invoke unauthorized tools | `eaeh-tools/tests/gateway.rs::autonomy_and_allowlist_enforced`, `::external_api_denied_by_default_policy`, `eaeh-agents/tests/agents.rs::prompt_injection_in_tool_output_cannot_escalate` | ok |
| Agent cannot access another project's data | `gateway.rs::cross_project_access_denied`, `eaeh-api/tests/api.rs::cross_project_isolation_and_role_checks`, `context::tests::scope_expiry_conflicts_budget_and_redaction`, `workspace::tests::scope_mismatch_and_cleanup` | ok |
| Prompt injection cannot authorize prohibited actions | `agents.rs::prompt_injection_in_tool_output_cannot_escalate`, `context::tests::untrusted_content_cannot_alter_instructions` | ok |
| Approval gates cannot be bypassed via direct API calls | `api.rs::approval_gate_cannot_be_bypassed_through_the_api`, `engine.rs::gate_cannot_be_bypassed_by_graph`, `cli.rs::cli_end_to_end` | ok |
| Agents cannot self-approve | `approval::tests::self_approval_and_non_humans_rejected`, `engine.rs::gate_blocks_until_human_approval_and_enforces_sod`, `gateway.rs::high_risk_tool_requires_bound_human_approval` | ok |
| Duplicate events do not cause duplicate side effects | `dispatcher::tests::duplicate_publish_and_redispatch_deliver_once`, `e2e.rs::agentic_workflow_with_planning_gate_and_full_traceability`, `idempotency::tests::first_result_wins` | ok |
| Interrupted workflows recover correctly | `engine.rs::interrupted_workflow_recovers_without_duplicate_side_effects` | ok |
| Execution budgets are enforced | `budget::tests::consumption_is_bounded_atomic_and_fail_closed`, `gateway.rs::budgets_enforced_and_fail_closed`, `model_client.rs::all_failing_and_budget_limits`, `model_client.rs::cost_budget_is_enforced_independently_of_tokens`, `agents.rs::bounded_correction_loop_detection_and_step_budget`, `agents.rs::depth_limit_enforced`, `harness.rs::supervision_escalates_stalled_work`, `harness.rs::supervision_escalates_repeated_failures` | ok |
| Secrets not exposed in logs or agent context | `e2e.rs::secrets_never_reach_audit_events_or_agent_context`, `logging.rs::logs_are_redacted_and_carry_correlation_ids`, `observability::tests::log_lines_are_redacted`, `audit::tests::details_are_redacted_before_storage`, `gateway.rs::tool_output_secrets_are_redacted_and_untrusted` | ok |
| Plugins cannot bypass policies | `harness.rs::plugins_require_admin_enablement_grants_and_cannot_bypass_policy`, `plugin::tests::lifecycle_grants_and_phase1_kind_restriction` | ok |
| Hook failure follows its defined error policy | `hooks::tests::failure_policies`, `hooks::tests::veto_only_honoured_at_before_points`, `gateway.rs::mandatory_controls_hold_when_hooks_missing_disabled_or_failing` | ok |
| Untrusted tool output cannot change system instructions | `agents.rs::prompt_injection_in_tool_output_cannot_escalate`, `context::tests::untrusted_content_cannot_alter_instructions` | ok |
| Concurrent workspace changes are controlled | `workspace::tests::concurrent_holders_conflict`, `workspace::tests::write_requires_lock_and_read_back` | ok |
| Model/provider failures follow fallback policies | `model_client.rs::falls_back_on_retryable_failure_and_records_versions`, `::non_retryable_errors_do_not_fall_back` | ok |
| Isolation boundaries actually hold | `container_isolation.rs::*` (7), `process_isolation.rs::*` (6) | ok |

## Developer-experience improvements → tests (2026-11-13)

Evidence: `evidence/dx-ci.log` (full gate), `evidence/dx-onboarding.log` (`scripts/demo-onboarding.sh`, simple-springboot-api, deterministic replay), `evidence/dx-extra-commands.log`.

| Requirement | Tests |
|---|---|
| Bootstrap and repeat execution | `dx.rs::bootstrap_is_one_command_repeatable_and_generates_claude_code_files` |
| Identity and permission assignment | same; `project.rs::readiness_enforces_responsibilities_and_isolation`; `identity::tests::grant_project_access_is_admin_only_tenant_bound_and_detailed_auth_reports_expiry` |
| Token lifecycle and authentication recovery | `dx.rs::identity_profiles_token_lifecycle_and_diagnostics`; `credentials::tests::selection_rules` |
| Skill discovery and registration | `project.rs::required_skills_are_discovered_and_synced_idempotently` |
| Approval authorization and bulk approval | `dx.rs::approval_review_needs_an_explicit_human_approver_and_confirms_each_item`; `project.rs::readiness_is_ready_after_human_approvals_with_a_model` |
| Readiness checks; missing provider and missing skill diagnostics | `project.rs::readiness_reports_missing_provider_and_unapproved_skills_with_fixes`; `dx.rs::readiness_and_workflow_prerequisites_explain_what_is_missing` |
| Workflow prerequisite validation | `dx.rs::readiness_and_workflow_prerequisites_…`; `project.rs::readiness_is_ready_after_…` (Phase 4 activities refused; identity without workflow.start blocked) |
| Cross-project and cross-tenant isolation | `project.rs::readiness_enforces_responsibilities_and_isolation`; `identity::tests::grant_project_access_…` |
| Claude Code configuration generation | `dx.rs::bootstrap_is_one_command_…` (CLAUDE.md preserved/merged, `.mcp.json`, no secrets); `dx.rs::mcp_server_exposes_governed_tools_without_approval_powers` |
| End-to-end deterministic onboarding and requirements workflow | `dx.rs::deterministic_onboarding_and_requirements_workflow`; `replay::tests::replays_in_order_per_agent_and_is_labelled`; `scripts/demo-onboarding.sh` (BRD → requirements → HLD → LLD) |

Not covered: a real Claude Code client session (A-44); multi-process concurrent CLI/MCP/server writers (A-20); OS keychain (A-43).

## Phase 3 acceptance criteria → tests

| AC | Tests (file::name) | Result in evidence |
|---|---|---|
| AC3-02 layering of new crates | `fitness.rs::crate_dependencies_follow_the_layering`, `::design_models_are_deterministic_and_independent_of_intelligence`, `::governance_is_separated_from_intelligent_orchestration` | ok |
| AC3-03 BRD as scoped, classified, citable knowledge | `design_demo.rs::brds_are_project_scoped_and_classification_bounded`, `cli.rs::phase3_brd_request_changes_trace_and_export` | ok |
| AC3-04 requirements model & validation | `requirements.rs::well_formed_set_has_no_blocking_findings_and_full_traceability`, `::structural_defects_are_blocking`, `model::tests::citation_and_id_formats` | ok |
| AC3-05 ambiguity / conflict / missing detection (+ baseline) | `requirements.rs::ambiguity_is_detected_and_acknowledged_items_are_downgraded`, `::conflicts_duplicates_and_numeric_conflicts_are_detected`, `::uncovered_brd_sections_are_missing_requirements`, `analysis::tests::*`, `analyzer_eval.rs::analyzer_precision_recall_baseline` | ok |
| AC3-06 BRD → requirement traceability | `requirements.rs::well_formed_set_…`, `architecture.rs::trace_links_connect_requirements_to_design_elements`, `design_demo.rs::brd_to_requirements_hld_and_lld_with_gates_revisions_and_traceability` | ok |
| AC3-07 HLD validation | `architecture.rs::hld_integrity_coverage_decisions_and_risks_are_enforced`, `::demonstration_fixtures_pass_all_deterministic_checks` | ok |
| AC3-08 standards compliance | `architecture.rs::mandatory_standards_must_be_addressed_and_deviations_need_an_adr`, `design_flow.rs::quality_gate_fails_closed_before_anything_is_stored` (platform-supplied standards) | ok |
| AC3-09 deterministic rendering, injection-safe diagrams | `architecture.rs::rendering_is_deterministic_and_complete`, `::model_text_cannot_inject_into_diagrams_or_documents`, `mermaid::tests::labels_and_identifiers_cannot_break_out` | ok |
| AC3-10 LLD validation / refinement | `architecture.rs::lld_must_refine_the_approved_hld_and_stay_consistent`, `::validators_report_deterministically_with_context` | ok |
| AC3-11 hash-bound gates, SoD, no API bypass | `design_flow.rs::bundles_bind_all_files_to_one_approval_and_trace_is_recorded_on_approval`, `api.rs::request_changes_and_traceability_routes_are_governed`, `design_demo.rs::brd_to_…` (skills: steward cannot approve), Phase 1 gate tests | ok |
| AC3-12 revision and rejection | `design_flow.rs::request_changes_revises_with_feedback_and_is_bounded`, `::reject_is_final_and_gates_without_a_revision_transition_fall_back`, `approval::tests::request_changes_is_limited_to_workflow_gates_and_needs_comments`, `cli.rs::phase3_…` | ok |
| AC3-13 quality gates fail closed; validator-driven correction | `design_flow.rs::quality_gate_fails_closed_before_anything_is_stored`, `::bindings_resolve_outputs_inputs_and_artifacts`, `requirements.rs::validator_runs_in_a_fail_closed_quality_gate`, `design_demo.rs::brd_to_…` (correction with `[requirements.set]` feedback) | ok |
| AC3-14 traceability store and query | `design_flow.rs::bundles_bind_…`, `design_demo.rs::brd_to_…`, `api.rs::request_changes_and_traceability_routes_are_governed` | ok |
| AC3-15 BRD injection | `design_demo.rs::injected_brd_instructions_cannot_change_instructions_tools_or_approvals` | ok |
| AC3-16 provenance of generated artifacts | `design_demo.rs::brd_to_…` (standards, skills, quality gates in provenance; skill/knowledge refs in model usage) | ok |
| AC3-17 API/CLI and design tools | `api.rs::request_changes_and_traceability_routes_are_governed`, `cli.rs::phase3_brd_request_changes_trace_and_export`, `tools.rs::design_check_reports_findings_for_drafts`, `::render_tools_produce_bundles_and_trace_links`; `phase-03-design-demo.log` | ok |
| AC3-18 demo end to end | `design_demo.rs::brd_to_requirements_hld_and_lld_with_gates_revisions_and_traceability` | ok (scripted model) |
| AC3-20 live run | — | **not executed** (no API key supplied) |

### Requirements-analyzer baseline (`phase-03-analyzer-eval.log`)

| Set | Result | Misses (recorded) |
|---|---|---|
| 24 statements, ambiguity | precision 0.923, recall 0.857 (tp 12, fp 1, fn 2, tn 9) | missed "timely", "large"; false positive "fast-track" |
| 8 pairs, conflict/duplicate/none | accuracy 0.875 | semantic conflict (guest checkout vs mandatory registration) |

Labels were written by the implementer (not independent; A-37). Regression guards: precision ≥ 0.9, recall ≥ 0.8, pair accuracy ≥ 0.85.

## Phase 2 acceptance criteria → tests

| AC | Tests (file::name) | Result in evidence |
|---|---|---|
| AC2-02 fitness for new crates | `fitness.rs::crate_dependencies_follow_the_layering`, `::http_client_and_provider_are_confined`, `::governed_context_constructors_have_single_producers`, `::no_credentials_in_configuration_and_prompts_are_assets` | ok |
| AC2-03 ingestion, chunking, versioning | `parse::tests::*` (3), `knowledge.rs::reference_documents_are_indexed_and_searchable_with_citations`, `::versions_supersede_and_retire_and_delete_purges` | ok |
| AC2-04 quarantine | `knowledge.rs::documents_with_credentials_are_quarantined_and_never_retrievable` | ok |
| AC2-05 lifecycle | `knowledge.rs::versions_supersede_and_retire_and_delete_purges` | ok |
| AC2-06 standards approval + SoD | `knowledge.rs::standards_activate_only_after_human_approval_bound_to_hash`, `::rejected_standards_never_activate`, `authz::tests::knowledge_steward_cannot_approve_and_agents_only_read_knowledge`, `api.rs::knowledge_and_skill_routes_are_governed` | ok |
| AC2-07 lexical/vector/hybrid, FTS injection | `search::tests::fts_query_is_neutralised`, `::rrf_rewards_agreement`, `embedding::tests::deterministic_normalised_and_similarity_ordered`, `knowledge.rs::fts_injection_cannot_break_queries_or_scope`, `retrieval_eval.rs::recall_baseline` | ok |
| AC2-08 no scope leakage | `knowledge.rs::scope_tenant_and_classification_isolation`, `::agent_retrieval_is_project_bound_and_ceiling_limited`, `api.rs::knowledge_and_skill_routes_are_governed` | ok |
| AC2-09 classification ceilings / egress | `classification::tests::ordering_and_parsing`, `policy::tests::restricted_content_never_egresses_to_external_models`, `model_client.rs::restricted_content_is_not_sent_to_external_providers`, `knowledge_agents.rs::restricted_knowledge_is_never_sent_to_external_models` | ok |
| AC2-10 mandatory standards precedence | `knowledge_agents.rs::agent_applies_standards_approved_skill_and_cited_knowledge`, `::injected_knowledge_cannot_override_standards_or_instructions`, `knowledge.rs::conflicts_freshness_and_standard_precedence` | ok |
| AC2-11 freshness and conflicts | `knowledge.rs::conflicts_freshness_and_standard_precedence` | ok |
| AC2-12 skill versions, dependencies, cycles, contracts | `skills.rs::registration_validation`, `::dependency_cycles_are_rejected`, `::selection_is_deterministic_with_dependencies_first_and_lifecycle` | ok |
| AC2-13 skill approval, no permission extension | `skills.rs::skills_require_human_approval_bound_to_digest`, `::rejected_skills_never_become_usable`, `::skills_never_extend_agent_permissions`, `knowledge_agents.rs::skills_require_a_configured_registry_and_must_be_approved` | ok |
| AC2-14 deterministic selection; traceability | `skills.rs::selection_is_deterministic_with_dependencies_first_and_lifecycle`, `knowledge_agents.rs::agent_applies_standards_approved_skill_and_cited_knowledge`, `knowledge_demo.rs::agent_reviews_api_spec_with_approved_skill_and_enterprise_knowledge` | ok |
| AC2-15 fabricated citations | `knowledge_agents.rs::fabricated_citations_fail_validation` | ok |
| AC2-16 adapter wire format, errors, retries (mock) | `mock_api.rs::wire_format_headers_and_tool_round_trip`, `::retries_429_with_retry_after_then_succeeds`, `::error_mapping_and_no_credential_leakage`, `::configuration_is_validated` | ok (mock only) |
| AC2-17 credentials, opt-in | `harness.rs::anthropic_provider_is_opt_in_and_needs_a_secret`, `mock_api.rs::error_mapping_and_no_credential_leakage`, `fitness.rs::no_credentials_in_configuration_and_prompts_are_assets` | ok |
| AC2-18 hooks, events, audit | `knowledge.rs::knowledge_hooks_observe_and_can_veto_retrieval` (before/after hooks, veto), `knowledge.rs::reference_documents_are_indexed_and_searchable_with_citations` (audit + `knowledge.*` events), `knowledge_agents.rs::agent_applies_standards_approved_skill_and_cited_knowledge` (agent audit refs) | ok |
| AC2-19 API/CLI governed | `api.rs::knowledge_and_skill_routes_are_governed`; CLI exercised by `scripts/demo-knowledge.sh` | ok / demo log |
| AC2-20 demo | `knowledge_demo.rs::agent_reviews_api_spec_with_approved_skill_and_enterprise_knowledge`; `phase-02-knowledge-demo.log` | ok (scripted model) |
| AC2-21 retrieval baseline | `retrieval_eval.rs::recall_baseline`; `phase-02-retrieval-eval.log` | ok |

### Retrieval-quality baseline (fixed set, `phase-02-retrieval-eval.log`)

| Set | Lexical recall@3 | Vector recall@3 | Hybrid recall@3 | Asserted? |
|---|---|---|---|---|
| 15 queries with lexical overlap | 1.000 (recall@1 1.000) | 1.000 (recall@1 1.000) | 1.000 (recall@1 1.000) | yes (≥ thresholds in test) |
| 5 paraphrase queries | 0.600 | 0.600 | 0.800 | no – recorded only |

The built-in embedder is hashed n-grams (not semantic); the paraphrase numbers are the honest
baseline that a semantic `EmbeddingProvider` must improve (A-29).

## Suites

| Suite | Type | Prerequisites / configuration | Sample data | Command | Acceptance criteria |
|---|---|---|---|---|---|

| `crates/eaeh-core` | Unit | none | in-test literals | `cargo test -p eaeh-core` | AC-01 |
| `crates/eaeh-store` | Unit/component | temp dir (tempfile) | temporary SQLite DB | `cargo test -p eaeh-store` | AC-03, AC-25, AC-26 |
| `crates/eaeh-events` | Component | temp SQLite DB | scripted subscribers | `cargo test -p eaeh-events` | AC-16 |
| `crates/eaeh-governance` | Unit/component (security) | temp SQLite DB | principals built in-test | `cargo test -p eaeh-governance` | AC-11, AC-12, AC-18, AC-24 |
| `crates/eaeh-extensibility` | Unit/component | temp SQLite DB; generated ed25519 keys | recording hooks/middleware | `cargo test -p eaeh-extensibility` | AC-13, AC-15, AC-17 |
| `crates/eaeh-isolation/src` | Unit | none | — | `cargo test -p eaeh-isolation --lib` | AC-07, AC-09 |
| `crates/eaeh-isolation/tests/process` | Isolation (security) | macOS/Linux with /bin/sh, /usr/bin/env, /bin/dd, /usr/bin/yes | shell one-liners | `cargo test -p eaeh-isolation --test process_isolation` | AC-07 |
| `crates/eaeh-isolation/tests/container` | Isolation (security, real escape attempts) | Podman (default) or Docker running; image docker.io/library/alpine:3.20 pulled; EAEH_CONTAINER_RUNTIME / EAEH_CONTAINER_IMAGE optional | shell escape scripts | `cargo test -p eaeh-isolation --test container_isolation` | AC-08 |
| `crates/eaeh-workspace` | Component (security) | temp dir | files, symlinks, locks | `cargo test -p eaeh-workspace` | AC-23 |
| `crates/eaeh-validation` | Unit | none | JSON subjects | `cargo test -p eaeh-validation` | AC-22 |
| `crates/eaeh-tools` | Integration (security & governance) | temp DB + workspace; restricted-process provider only | probe tools with configurable risk | `cargo test -p eaeh-tools` | AC-09, AC-10, AC-14, AC-18, AC-24, AC-25 |
| `crates/eaeh-intelligence/src` | Unit (security) | temp DB | prompt templates, injected text | `cargo test -p eaeh-intelligence --lib` | AC-19, AC-20 |
| `crates/eaeh-intelligence/tests` | Integration | temp DB; in-test fake providers | model descriptors with costs | `cargo test -p eaeh-intelligence --test model_client` | AC-21 |
| `crates/eaeh-agents` | Integration (security) | full governed stack; ScriptedModelProvider (test double) | scripted model responses, sample agent definitions | `cargo test -p eaeh-agents` | AC-05, AC-06, AC-10, AC-18, AC-19 |
| `crates/eaeh-workflow` | Integration (governance, recovery) | temp DB; fake TaskExecutor/Planner | JSON workflow definitions in test | `cargo test -p eaeh-workflow` | AC-03, AC-04, AC-11 |
| `crates/eaeh-runtime` | Integration (composition root) | shipped config/eaeh.toml with temp data_dir; container disabled where noted | shipped prompts/agents/workflows | `cargo test -p eaeh-runtime` | AC-17, AC-18, AC-27 |
| `crates/eaeh-api` | API contract & security | in-process router (tower oneshot); shipped definitions | bootstrapped tenant, users and tokens | `cargo test -p eaeh-api` | AC-11, AC-24, AC-27, AC-28 |
| `crates/eaeh-cli` | CLI end-to-end | compiled `eaeh` binary; generated config | bootstrap + demo.approval | `cargo test -p eaeh-cli` | AC-28, AC-26 |
| `tests/tests/fitness` | Architecture fitness | `cargo metadata --offline`; source tree | — | `cargo test -p eaeh-acceptance --test fitness` | AC-02 |
| `tests/tests/logging` | Logging (redaction, correlation) | own process; global JSON subscriber at DEBUG; shipped config, container disabled | scripted agent run reading a file containing a secret | `cargo test -p eaeh-acceptance --test logging` | AC-25, AC-27 |
| `crates/eaeh-knowledge` | Unit + integration (security, retrieval) | temp SQLite DB with FTS5; bootstrapped tenant/projects/principals | in-test Markdown/JSON/OpenAPI documents | `cargo test -p eaeh-knowledge` | AC2-03…AC2-11, AC2-18, AC2-21 |
| `crates/eaeh-skills` | Integration (governance) | temp DB; approval manager | in-test skill definitions | `cargo test -p eaeh-skills` | AC2-12…AC2-14 |
| `crates/eaeh-agents/tests/knowledge_agents` | Integration (security, RAG) | full governed stack + knowledge + skills; ScriptedModelProvider (test double) | scripted model responses, injected documents | `cargo test -p eaeh-agents --test knowledge_agents` | AC2-09, AC2-10, AC2-13…AC2-15 |
| `crates/eaeh-provider-anthropic` | Contract vs. local mock HTTP server (axum on 127.0.0.1) | no network; dummy key | recorded request/response shapes | `cargo test -p eaeh-provider-anthropic` | AC2-16, AC2-17 |
| `tests/tests/knowledge_demo` | End-to-end demo | full harness; shipped config; `examples/knowledge`, `config/skills`; ScriptedModelProvider | demonstration corpus (labelled, not real standards) | `cargo test -p eaeh-acceptance --test knowledge_demo` | AC2-20 |
| `crates/eaeh-requirements` | Unit + integration + evaluation (deterministic) | none (pure) | in-test requirement sets; labelled fixture statements | `cargo test -p eaeh-requirements` | AC3-04…AC3-06 |
| `crates/eaeh-architecture` | Unit + integration (deterministic, security) | none (pure) | `examples/design/{requirements,hld,lld}.json` (labelled demonstration) | `cargo test -p eaeh-architecture` | AC3-07…AC3-10 |
| `crates/eaeh-workflow/tests/design_flow` | Integration (governance) | temp DB; scripted executor; test validator | scripted outputs and bundles | `cargo test -p eaeh-workflow --test design_flow` | AC3-11…AC3-14 |
| `crates/eaeh-engineering` | Tool contract | none | `examples/design` | `cargo test -p eaeh-engineering` | AC3-09, AC3-17 |
| `tests/tests/design_demo` | End-to-end demo + security | full harness, shipped config; ScriptedModelProvider | `examples/brd`, `examples/design`, `config/skills` | `cargo test -p eaeh-acceptance --test design_demo` | AC3-03, AC3-11…AC3-18 |
| `tests/tests/e2e` | End-to-end (security, traceability, isolation) | full harness; ScriptedModelProvider; Podman for container test; data dir under target/tmp | scripted agentic run, test secrets provider | `cargo test -p eaeh-acceptance --test e2e` | AC-03, AC-04, AC-08, AC-16, AC-25, AC-26 |
| `tests/tests/external_ai` | End-to-end (external AI execution, ADR-0025; security, no-fallback, delivery) | full harness on the shipped config in `external_ai` mode; a ScriptedModelProvider is *offered* to prove it is never registered or called | `examples/brd`, `examples/design`, `config/skills`; the test acts as the AI client | `cargo test -p eaeh-acceptance --test external_ai` | hand-off lifecycle BRD→LLD with 0 model calls; correction budget; project isolation; submitter cannot approve; direct-provider runtime refuses a handed-off task; deterministic mode registers replay only; delivery plan→approval→workspace→bundle→approval→traces; required build step never skipped |
| `crates/eaeh-cli/tests/dx` (`external_ai_bootstrap_cli_and_mcp_hand_offs_without_model_calls`) | CLI + MCP end-to-end | compiled binary; `external_ai` config with replay enabled (must be ignored) | `examples/simple-springboot-api` + cassette JSON used as client submissions | `cargo test -p eaeh-cli --test dx external_ai` | default bootstrap (both clients, delivery); no tokens in generated files; readiness reports mode and clients; CLI hand-off; MCP hand-off as a Copilot-labelled client; provenance per client; no `model.invoke` |
| `eaeh-engineering::delivery`, `eaeh-agents::external` (unit) | Unit | none | in-test JSON | `cargo test -p eaeh-engineering -p eaeh-agents --lib` | delivery plan/implementation/build validators; path safety; mode parsing; client labels |

## Test catalogue (all 214 tests)

Phase column: phase in which the test was added (`1`: in `phase-01-ci.log`; `2`: in `phase-02-ci.log`; `3`: new). Every row appears as `ok` in `phase-03-ci.log` (checked by the generator against the source tree).

| # | Phase | File | Test | Objective (from name) | Suite command |
|---|---|---|---|---|---|
| 1 | 1 | `crates/eaeh-agents/tests/agents.rs` | `registration_rules_and_smallest_effective_discovery` | Registration rules and smallest effective discovery | `cargo test -p eaeh-agents --test agents` |
| 2 | 1 | `crates/eaeh-agents/tests/agents.rs` | `dynamic_lifecycle_with_tools_hooks_and_events` | Dynamic lifecycle with tools hooks and events | `cargo test -p eaeh-agents --test agents` |
| 3 | 1 | `crates/eaeh-agents/tests/agents.rs` | `prompt_injection_in_tool_output_cannot_escalate` | Prompt injection in tool output cannot escalate | `cargo test -p eaeh-agents --test agents` |
| 4 | 1 | `crates/eaeh-agents/tests/agents.rs` | `bounded_correction_loop_detection_and_step_budget` | Bounded correction loop detection and step budget | `cargo test -p eaeh-agents --test agents` |
| 5 | 1 | `crates/eaeh-agents/tests/agents.rs` | `delegation_controls` | Delegation controls | `cargo test -p eaeh-agents --test agents` |
| 6 | 1 | `crates/eaeh-agents/tests/agents.rs` | `depth_limit_enforced` | Depth limit enforced | `cargo test -p eaeh-agents --test agents` |
| 7 | 2 | `crates/eaeh-agents/tests/knowledge_agents.rs` | `agent_applies_standards_approved_skill_and_cited_knowledge` | Agent applies standards approved skill and cited knowledge | `cargo test -p eaeh-agents --test knowledge_agents` |
| 8 | 2 | `crates/eaeh-agents/tests/knowledge_agents.rs` | `fabricated_citations_fail_validation` | Fabricated citations fail validation | `cargo test -p eaeh-agents --test knowledge_agents` |
| 9 | 2 | `crates/eaeh-agents/tests/knowledge_agents.rs` | `injected_knowledge_cannot_override_standards_or_instructions` | Injected knowledge cannot override standards or instructions | `cargo test -p eaeh-agents --test knowledge_agents` |
| 10 | 2 | `crates/eaeh-agents/tests/knowledge_agents.rs` | `restricted_knowledge_is_never_sent_to_external_models` | Restricted knowledge is never sent to external models | `cargo test -p eaeh-agents --test knowledge_agents` |
| 11 | 2 | `crates/eaeh-agents/tests/knowledge_agents.rs` | `skills_require_a_configured_registry_and_must_be_approved` | Skills require a configured registry and must be approved | `cargo test -p eaeh-agents --test knowledge_agents` |
| 12 | 1 | `crates/eaeh-api/tests/api.rs` | `authentication_and_error_contract` | Authentication and error contract | `cargo test -p eaeh-api --test api` |
| 13 | 1 | `crates/eaeh-api/tests/api.rs` | `approval_gate_cannot_be_bypassed_through_the_api` | Approval gate cannot be bypassed through the api | `cargo test -p eaeh-api --test api` |
| 14 | 1 | `crates/eaeh-api/tests/api.rs` | `cross_project_isolation_and_role_checks` | Cross project isolation and role checks | `cargo test -p eaeh-api --test api` |
| 15 | 1 | `crates/eaeh-api/tests/api.rs` | `pagination_tokens_and_rate_limits` | Pagination tokens and rate limits | `cargo test -p eaeh-api --test api` |
| 16 | 1 | `crates/eaeh-api/tests/api.rs` | `metrics_and_health_details_require_permission` | Metrics and health details require permission | `cargo test -p eaeh-api --test api` |
| 17 | 2 | `crates/eaeh-api/tests/api.rs` | `knowledge_and_skill_routes_are_governed` | Knowledge and skill routes are governed | `cargo test -p eaeh-api --test api` |
| 18 | 3 | `crates/eaeh-api/tests/api.rs` | `request_changes_and_traceability_routes_are_governed` | Request changes and traceability routes are governed | `cargo test -p eaeh-api --test api` |
| 19 | 3 | `crates/eaeh-architecture/src/mermaid.rs` | `labels_and_identifiers_cannot_break_out` | Labels and identifiers cannot break out | `cargo test -p eaeh-architecture --lib` |
| 20 | 3 | `crates/eaeh-architecture/tests/architecture.rs` | `demonstration_fixtures_pass_all_deterministic_checks` | Demonstration fixtures pass all deterministic checks | `cargo test -p eaeh-architecture --test architecture` |
| 21 | 3 | `crates/eaeh-architecture/tests/architecture.rs` | `hld_integrity_coverage_decisions_and_risks_are_enforced` | Hld integrity coverage decisions and risks are enforced | `cargo test -p eaeh-architecture --test architecture` |
| 22 | 3 | `crates/eaeh-architecture/tests/architecture.rs` | `mandatory_standards_must_be_addressed_and_deviations_need_an_adr` | Mandatory standards must be addressed and deviations need an adr | `cargo test -p eaeh-architecture --test architecture` |
| 23 | 3 | `crates/eaeh-architecture/tests/architecture.rs` | `lld_must_refine_the_approved_hld_and_stay_consistent` | Lld must refine the approved hld and stay consistent | `cargo test -p eaeh-architecture --test architecture` |
| 24 | 3 | `crates/eaeh-architecture/tests/architecture.rs` | `rendering_is_deterministic_and_complete` | Rendering is deterministic and complete | `cargo test -p eaeh-architecture --test architecture` |
| 25 | 3 | `crates/eaeh-architecture/tests/architecture.rs` | `model_text_cannot_inject_into_diagrams_or_documents` | Model text cannot inject into diagrams or documents | `cargo test -p eaeh-architecture --test architecture` |
| 26 | 3 | `crates/eaeh-architecture/tests/architecture.rs` | `trace_links_connect_requirements_to_design_elements` | Trace links connect requirements to design elements | `cargo test -p eaeh-architecture --test architecture` |
| 27 | 3 | `crates/eaeh-architecture/tests/architecture.rs` | `validators_report_deterministically_with_context` | Validators report deterministically with context | `cargo test -p eaeh-architecture --test architecture` |
| 28 | 1 | `crates/eaeh-cli/tests/cli.rs` | `cli_end_to_end` | Cli end to end | `cargo test -p eaeh-cli --test cli` |
| 29 | 3 | `crates/eaeh-cli/tests/cli.rs` | `phase3_brd_request_changes_trace_and_export` | Phase3 brd request changes trace and export | `cargo test -p eaeh-cli --test cli` |
| 30 | 2 | `crates/eaeh-core/src/classification.rs` | `ordering_and_parsing` | Ordering and parsing | `cargo test -p eaeh-core --lib` |
| 31 | 1 | `crates/eaeh-core/src/error.rs` | `retryability_is_limited_to_transient_kinds` | Retryability is limited to transient kinds | `cargo test -p eaeh-core --lib` |
| 32 | 1 | `crates/eaeh-core/src/error.rs` | `context_preserves_kind` | Context preserves kind | `cargo test -p eaeh-core --lib` |
| 33 | 1 | `crates/eaeh-core/src/hash.rs` | `hex_roundtrip_and_known_digest` | Hex roundtrip and known digest | `cargo test -p eaeh-core --lib` |
| 34 | 1 | `crates/eaeh-core/src/hash.rs` | `canonical_json_is_key_order_independent` | Canonical json is key order independent | `cargo test -p eaeh-core --lib` |
| 35 | 1 | `crates/eaeh-core/src/hash.rs` | `random_bytes_differ` | Random bytes differ | `cargo test -p eaeh-core --lib` |
| 36 | 1 | `crates/eaeh-core/src/ids.rs` | `uuid_ids_roundtrip_and_are_time_ordered` | Uuid ids roundtrip and are time ordered | `cargo test -p eaeh-core --lib` |
| 37 | 1 | `crates/eaeh-core/src/ids.rs` | `slugs_reject_traversal_and_uppercase` | Slugs reject traversal and uppercase | `cargo test -p eaeh-core --lib` |
| 38 | 1 | `crates/eaeh-core/src/principal.rs` | `agent_principals_have_no_roles_and_single_project` | Agent principals have no roles and single project | `cargo test -p eaeh-core --lib` |
| 39 | 1 | `crates/eaeh-core/src/principal.rs` | `tenant_boundary_applies_even_with_all_projects` | Tenant boundary applies even with all projects | `cargo test -p eaeh-core --lib` |
| 40 | 1 | `crates/eaeh-core/src/risk.rs` | `autonomy_maps_to_monotonic_risk_ceiling` | Autonomy maps to monotonic risk ceiling | `cargo test -p eaeh-core --lib` |
| 41 | 1 | `crates/eaeh-core/src/secret.rs` | `secret_never_formats_value` | Secret never formats value | `cargo test -p eaeh-core --lib` |
| 42 | 1 | `crates/eaeh-core/src/secret.rs` | `redactor_removes_registered_and_pattern_secrets` | Redactor removes registered and pattern secrets | `cargo test -p eaeh-core --lib` |
| 43 | 1 | `crates/eaeh-core/src/secret.rs` | `redactor_handles_json_and_sensitive_keys` | Redactor handles json and sensitive keys | `cargo test -p eaeh-core --lib` |
| 44 | 1 | `crates/eaeh-core/src/secret.rs` | `env_provider_is_prefix_restricted` | Env provider is prefix restricted | `cargo test -p eaeh-core --lib` |
| 45 | 3 | `crates/eaeh-engineering/tests/tools.rs` | `design_check_reports_findings_for_drafts` | Design check reports findings for drafts | `cargo test -p eaeh-engineering --test tools` |
| 46 | 3 | `crates/eaeh-engineering/tests/tools.rs` | `render_tools_produce_bundles_and_trace_links` | Render tools produce bundles and trace links | `cargo test -p eaeh-engineering --test tools` |
| 47 | 1 | `crates/eaeh-events/src/dispatcher.rs` | `duplicate_publish_and_redispatch_deliver_once` | Duplicate publish and redispatch deliver once | `cargo test -p eaeh-events --lib` |
| 48 | 1 | `crates/eaeh-events/src/dispatcher.rs` | `transient_failures_are_retried_then_delivered` | Transient failures are retried then delivered | `cargo test -p eaeh-events --lib` |
| 49 | 1 | `crates/eaeh-events/src/dispatcher.rs` | `persistent_failure_is_dead_lettered_and_audited` | Persistent failure is dead lettered and audited | `cargo test -p eaeh-events --lib` |
| 50 | 1 | `crates/eaeh-events/src/dispatcher.rs` | `ordered_consumer_does_not_skip_ahead` | Ordered consumer does not skip ahead | `cargo test -p eaeh-events --lib` |
| 51 | 1 | `crates/eaeh-events/src/dispatcher.rs` | `backoff_grows_and_is_capped` | Backoff grows and is capped | `cargo test -p eaeh-events --lib` |
| 52 | 1 | `crates/eaeh-extensibility/src/hooks.rs` | `ordering_by_priority_then_id` | Ordering by priority then id | `cargo test -p eaeh-extensibility --lib` |
| 53 | 1 | `crates/eaeh-extensibility/src/hooks.rs` | `failure_policies` | Failure policies | `cargo test -p eaeh-extensibility --lib` |
| 54 | 1 | `crates/eaeh-extensibility/src/hooks.rs` | `veto_only_honoured_at_before_points` | Veto only honoured at before points | `cargo test -p eaeh-extensibility --lib` |
| 55 | 1 | `crates/eaeh-extensibility/src/hooks.rs` | `filters_disable_and_compatibility` | Filters disable and compatibility | `cargo test -p eaeh-extensibility --lib` |
| 56 | 1 | `crates/eaeh-extensibility/src/interceptor.rs` | `chain_applies_in_order_and_stops_on_error` | Chain applies in order and stops on error | `cargo test -p eaeh-extensibility --lib` |
| 57 | 1 | `crates/eaeh-extensibility/src/middleware.rs` | `deterministic_onion_order` | Deterministic onion order | `cargo test -p eaeh-extensibility --lib` |
| 58 | 1 | `crates/eaeh-extensibility/src/middleware.rs` | `short_circuit_and_errors_propagate` | Short circuit and errors propagate | `cargo test -p eaeh-extensibility --lib` |
| 59 | 1 | `crates/eaeh-extensibility/src/middleware.rs` | `pipeline_timeout` | Pipeline timeout | `cargo test -p eaeh-extensibility --lib` |
| 60 | 1 | `crates/eaeh-extensibility/src/plugin.rs` | `signature_verification` | Signature verification | `cargo test -p eaeh-extensibility --lib` |
| 61 | 1 | `crates/eaeh-extensibility/src/plugin.rs` | `lifecycle_grants_and_phase1_kind_restriction` | Lifecycle grants and phase1 kind restriction | `cargo test -p eaeh-extensibility --lib` |
| 62 | 1 | `crates/eaeh-governance/src/approval.rs` | `approver_can_approve_with_matching_hash` | Approver can approve with matching hash | `cargo test -p eaeh-governance --lib` |
| 63 | 1 | `crates/eaeh-governance/src/approval.rs` | `self_approval_and_non_humans_rejected` | Self approval and non humans rejected | `cargo test -p eaeh-governance --lib` |
| 64 | 1 | `crates/eaeh-governance/src/approval.rs` | `artifact_hash_must_match` | Artifact hash must match | `cargo test -p eaeh-governance --lib` |
| 65 | 1 | `crates/eaeh-governance/src/approval.rs` | `rerequest_supersedes_and_keeps_history` | Rerequest supersedes and keeps history | `cargo test -p eaeh-governance --lib` |
| 66 | 3 | `crates/eaeh-governance/src/approval.rs` | `request_changes_is_limited_to_workflow_gates_and_needs_comments` | Request changes is limited to workflow gates and needs comments | `cargo test -p eaeh-governance --lib` |
| 67 | 1 | `crates/eaeh-governance/src/authz.rs` | `only_human_approvers_can_decide` | Only human approvers can decide | `cargo test -p eaeh-governance --lib` |
| 68 | 2 | `crates/eaeh-governance/src/authz.rs` | `knowledge_steward_cannot_approve_and_agents_only_read_knowledge` | Knowledge steward cannot approve and agents only read knowledge | `cargo test -p eaeh-governance --lib` |
| 69 | 1 | `crates/eaeh-governance/src/authz.rs` | `cross_tenant_and_cross_project_denied` | Cross tenant and cross project denied | `cargo test -p eaeh-governance --lib` |
| 70 | 1 | `crates/eaeh-governance/src/budget.rs` | `consumption_is_bounded_atomic_and_fail_closed` | Consumption is bounded atomic and fail closed | `cargo test -p eaeh-governance --lib` |
| 71 | 1 | `crates/eaeh-governance/src/budget.rs` | `child_limits_never_exceed_parent` | Child limits never exceed parent | `cargo test -p eaeh-governance --lib` |
| 72 | 1 | `crates/eaeh-governance/src/identity.rs` | `bootstrap_is_one_time_and_token_authenticates` | Bootstrap is one time and token authenticates | `cargo test -p eaeh-governance --lib` |
| 73 | 1 | `crates/eaeh-governance/src/identity.rs` | `revoke_rotate_and_reject_invalid` | Revoke rotate and reject invalid | `cargo test -p eaeh-governance --lib` |
| 74 | 1 | `crates/eaeh-governance/src/identity.rs` | `non_admin_cannot_manage_others_or_create_principals` | Non admin cannot manage others or create principals | `cargo test -p eaeh-governance --lib` |
| 75 | 1 | `crates/eaeh-governance/src/identity.rs` | `expired_tokens_rejected` | Expired tokens rejected | `cargo test -p eaeh-governance --lib` |
| 76 | 1 | `crates/eaeh-governance/src/policy.rs` | `baseline_decisions` | Baseline decisions | `cargo test -p eaeh-governance --lib` |
| 77 | 1 | `crates/eaeh-governance/src/policy.rs` | `project_policy_cannot_override_enterprise_deny` | Project policy cannot override enterprise deny | `cargo test -p eaeh-governance --lib` |
| 78 | 2 | `crates/eaeh-governance/src/policy.rs` | `restricted_content_never_egresses_to_external_models` | Restricted content never egresses to external models | `cargo test -p eaeh-governance --lib` |
| 79 | 1 | `crates/eaeh-governance/src/policy.rs` | `evaluation_is_deterministic` | Evaluation is deterministic | `cargo test -p eaeh-governance --lib` |
| 80 | 1 | `crates/eaeh-governance/src/policy.rs` | `invalid_documents_rejected` | Invalid documents rejected | `cargo test -p eaeh-governance --lib` |
| 81 | 1 | `crates/eaeh-intelligence/src/context.rs` | `untrusted_content_cannot_alter_instructions` | Untrusted content cannot alter instructions | `cargo test -p eaeh-intelligence --lib` |
| 82 | 1 | `crates/eaeh-intelligence/src/context.rs` | `scope_expiry_conflicts_budget_and_redaction` | Scope expiry conflicts budget and redaction | `cargo test -p eaeh-intelligence --lib` |
| 83 | 1 | `crates/eaeh-intelligence/src/context.rs` | `untrusted_cannot_be_constructed_as_instruction` | Untrusted cannot be constructed as instruction | `cargo test -p eaeh-intelligence --lib` |
| 84 | 2 | `crates/eaeh-intelligence/src/embedding.rs` | `deterministic_normalised_and_similarity_ordered` | Deterministic normalised and similarity ordered | `cargo test -p eaeh-intelligence --lib` |
| 85 | 1 | `crates/eaeh-intelligence/src/prompt.rs` | `validation_and_rendering` | Validation and rendering | `cargo test -p eaeh-intelligence --lib` |
| 86 | 1 | `crates/eaeh-intelligence/src/prompt.rs` | `approval_rollback_and_immutability` | Approval rollback and immutability | `cargo test -p eaeh-intelligence --lib` |
| 87 | 1 | `crates/eaeh-intelligence/tests/model_client.rs` | `routes_to_cheapest_capable_model_not_most_expensive` | Routes to cheapest capable model not most expensive | `cargo test -p eaeh-intelligence --test model_client` |
| 88 | 1 | `crates/eaeh-intelligence/tests/model_client.rs` | `falls_back_on_retryable_failure_and_records_versions` | Falls back on retryable failure and records versions | `cargo test -p eaeh-intelligence --test model_client` |
| 89 | 1 | `crates/eaeh-intelligence/tests/model_client.rs` | `non_retryable_errors_do_not_fall_back` | Non retryable errors do not fall back | `cargo test -p eaeh-intelligence --test model_client` |
| 90 | 1 | `crates/eaeh-intelligence/tests/model_client.rs` | `all_failing_and_budget_limits` | All failing and budget limits | `cargo test -p eaeh-intelligence --test model_client` |
| 91 | 1 | `crates/eaeh-intelligence/tests/model_client.rs` | `cost_budget_is_enforced_independently_of_tokens` | Cost budget is enforced independently of tokens | `cargo test -p eaeh-intelligence --test model_client` |
| 92 | 2 | `crates/eaeh-intelligence/tests/model_client.rs` | `restricted_content_is_not_sent_to_external_providers` | Restricted content is not sent to external providers | `cargo test -p eaeh-intelligence --test model_client` |
| 93 | 1 | `crates/eaeh-isolation/src/lib.rs` | `selection_refuses_instead_of_downgrading` | Selection refuses instead of downgrading | `cargo test -p eaeh-isolation --lib` |
| 94 | 1 | `crates/eaeh-isolation/src/lib.rs` | `process_provider_declares_honest_capabilities` | Process provider declares honest capabilities | `cargo test -p eaeh-isolation --lib` |
| 95 | 1 | `crates/eaeh-isolation/tests/container_isolation.rs` | `runs_as_unprivileged_user_without_capabilities` | Runs as unprivileged user without capabilities | `cargo test -p eaeh-isolation --test container_isolation` |
| 96 | 1 | `crates/eaeh-isolation/tests/container_isolation.rs` | `network_access_is_denied` | Network access is denied | `cargo test -p eaeh-isolation --test container_isolation` |
| 97 | 1 | `crates/eaeh-isolation/tests/container_isolation.rs` | `filesystem_is_confined_to_workspace` | Filesystem is confined to workspace | `cargo test -p eaeh-isolation --test container_isolation` |
| 98 | 1 | `crates/eaeh-isolation/tests/container_isolation.rs` | `memory_limit_is_enforced` | Memory limit is enforced | `cargo test -p eaeh-isolation --test container_isolation` |
| 99 | 1 | `crates/eaeh-isolation/tests/container_isolation.rs` | `process_count_limit_is_enforced` | Process count limit is enforced | `cargo test -p eaeh-isolation --test container_isolation` |
| 100 | 1 | `crates/eaeh-isolation/tests/container_isolation.rs` | `timeout_kills_and_removes_container` | Timeout kills and removes container | `cargo test -p eaeh-isolation --test container_isolation` |
| 101 | 1 | `crates/eaeh-isolation/tests/container_isolation.rs` | `environment_contains_only_explicit_variables` | Environment contains only explicit variables | `cargo test -p eaeh-isolation --test container_isolation` |
| 102 | 1 | `crates/eaeh-isolation/tests/process_isolation.rs` | `environment_is_cleared_to_allowlist` | Environment is cleared to allowlist | `cargo test -p eaeh-isolation --test process_isolation` |
| 103 | 1 | `crates/eaeh-isolation/tests/process_isolation.rs` | `timeout_kills_entire_process_group` | Timeout kills entire process group | `cargo test -p eaeh-isolation --test process_isolation` |
| 104 | 1 | `crates/eaeh-isolation/tests/process_isolation.rs` | `cpu_time_limit_terminates_busy_loop` | Cpu time limit terminates busy loop | `cargo test -p eaeh-isolation --test process_isolation` |
| 105 | 1 | `crates/eaeh-isolation/tests/process_isolation.rs` | `file_size_limit_enforced` | File size limit enforced | `cargo test -p eaeh-isolation --test process_isolation` |
| 106 | 1 | `crates/eaeh-isolation/tests/process_isolation.rs` | `output_is_bounded` | Output is bounded | `cargo test -p eaeh-isolation --test process_isolation` |
| 107 | 1 | `crates/eaeh-isolation/tests/process_isolation.rs` | `refuses_requirements_it_cannot_enforce` | Refuses requirements it cannot enforce | `cargo test -p eaeh-isolation --test process_isolation` |
| 108 | 2 | `crates/eaeh-knowledge/src/parse.rs` | `markdown_chunks_follow_heading_paths_and_skip_code_headings` | Markdown chunks follow heading paths and skip code headings | `cargo test -p eaeh-knowledge --lib` |
| 109 | 2 | `crates/eaeh-knowledge/src/parse.rs` | `long_sections_split_and_deterministic` | Long sections split and deterministic | `cargo test -p eaeh-knowledge --lib` |
| 110 | 2 | `crates/eaeh-knowledge/src/parse.rs` | `openapi_operations_and_schemas` | Openapi operations and schemas | `cargo test -p eaeh-knowledge --lib` |
| 111 | 2 | `crates/eaeh-knowledge/src/search.rs` | `fts_query_is_neutralised` | Fts query is neutralised | `cargo test -p eaeh-knowledge --lib` |
| 112 | 2 | `crates/eaeh-knowledge/src/search.rs` | `rrf_rewards_agreement` | Rrf rewards agreement | `cargo test -p eaeh-knowledge --lib` |
| 113 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `reference_documents_are_indexed_and_searchable_with_citations` | Reference documents are indexed and searchable with citations | `cargo test -p eaeh-knowledge --test knowledge` |
| 114 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `standards_activate_only_after_human_approval_bound_to_hash` | Standards activate only after human approval bound to hash | `cargo test -p eaeh-knowledge --test knowledge` |
| 115 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `rejected_standards_never_activate` | Rejected standards never activate | `cargo test -p eaeh-knowledge --test knowledge` |
| 116 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `documents_with_credentials_are_quarantined_and_never_retrievable` | Documents with credentials are quarantined and never retrievable | `cargo test -p eaeh-knowledge --test knowledge` |
| 117 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `scope_tenant_and_classification_isolation` | Scope tenant and classification isolation | `cargo test -p eaeh-knowledge --test knowledge` |
| 118 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `versions_supersede_and_retire_and_delete_purges` | Versions supersede and retire and delete purges | `cargo test -p eaeh-knowledge --test knowledge` |
| 119 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `conflicts_freshness_and_standard_precedence` | Conflicts freshness and standard precedence | `cargo test -p eaeh-knowledge --test knowledge` |
| 120 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `fts_injection_cannot_break_queries_or_scope` | Fts injection cannot break queries or scope | `cargo test -p eaeh-knowledge --test knowledge` |
| 121 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `agent_retrieval_is_project_bound_and_ceiling_limited` | Agent retrieval is project bound and ceiling limited | `cargo test -p eaeh-knowledge --test knowledge` |
| 122 | 2 | `crates/eaeh-knowledge/tests/knowledge.rs` | `knowledge_hooks_observe_and_can_veto_retrieval` | Knowledge hooks observe and can veto retrieval | `cargo test -p eaeh-knowledge --test knowledge` |
| 123 | 2 | `crates/eaeh-knowledge/tests/retrieval_eval.rs` | `recall_baseline` | Recall baseline | `cargo test -p eaeh-knowledge --test retrieval_eval` |
| 124 | 2 | `crates/eaeh-provider-anthropic/tests/mock_api.rs` | `wire_format_headers_and_tool_round_trip` | Wire format headers and tool round trip | `cargo test -p eaeh-provider-anthropic --test mock_api` |
| 125 | 2 | `crates/eaeh-provider-anthropic/tests/mock_api.rs` | `retries_429_with_retry_after_then_succeeds` | Retries 429 with retry after then succeeds | `cargo test -p eaeh-provider-anthropic --test mock_api` |
| 126 | 2 | `crates/eaeh-provider-anthropic/tests/mock_api.rs` | `error_mapping_and_no_credential_leakage` | Error mapping and no credential leakage | `cargo test -p eaeh-provider-anthropic --test mock_api` |
| 127 | 2 | `crates/eaeh-provider-anthropic/tests/mock_api.rs` | `configuration_is_validated` | Configuration is validated | `cargo test -p eaeh-provider-anthropic --test mock_api` |
| 128 | 3 | `crates/eaeh-requirements/src/analysis.rs` | `phrase_matching_is_whole_word` | Phrase matching is whole word | `cargo test -p eaeh-requirements --lib` |
| 129 | 3 | `crates/eaeh-requirements/src/analysis.rs` | `quantities_are_normalised` | Quantities are normalised | `cargo test -p eaeh-requirements --lib` |
| 130 | 3 | `crates/eaeh-requirements/src/model.rs` | `citation_and_id_formats` | Citation and id formats | `cargo test -p eaeh-requirements --lib` |
| 131 | 3 | `crates/eaeh-requirements/tests/analyzer_eval.rs` | `analyzer_precision_recall_baseline` | Analyzer precision recall baseline | `cargo test -p eaeh-requirements --test analyzer_eval` |
| 132 | 3 | `crates/eaeh-requirements/tests/requirements.rs` | `well_formed_set_has_no_blocking_findings_and_full_traceability` | Well formed set has no blocking findings and full traceability | `cargo test -p eaeh-requirements --test requirements` |
| 133 | 3 | `crates/eaeh-requirements/tests/requirements.rs` | `structural_defects_are_blocking` | Structural defects are blocking | `cargo test -p eaeh-requirements --test requirements` |
| 134 | 3 | `crates/eaeh-requirements/tests/requirements.rs` | `ambiguity_is_detected_and_acknowledged_items_are_downgraded` | Ambiguity is detected and acknowledged items are downgraded | `cargo test -p eaeh-requirements --test requirements` |
| 135 | 3 | `crates/eaeh-requirements/tests/requirements.rs` | `conflicts_duplicates_and_numeric_conflicts_are_detected` | Conflicts duplicates and numeric conflicts are detected | `cargo test -p eaeh-requirements --test requirements` |
| 136 | 3 | `crates/eaeh-requirements/tests/requirements.rs` | `uncovered_brd_sections_are_missing_requirements` | Uncovered brd sections are missing requirements | `cargo test -p eaeh-requirements --test requirements` |
| 137 | 3 | `crates/eaeh-requirements/tests/requirements.rs` | `validator_runs_in_a_fail_closed_quality_gate` | Validator runs in a fail closed quality gate | `cargo test -p eaeh-requirements --test requirements` |
| 138 | 1 | `crates/eaeh-runtime/src/observability.rs` | `log_lines_are_redacted` | Log lines are redacted | `cargo test -p eaeh-runtime --lib` |
| 139 | 1 | `crates/eaeh-runtime/tests/harness.rs` | `shipped_definitions_and_demo_workflow_end_to_end` | Shipped definitions and demo workflow end to end | `cargo test -p eaeh-runtime --test harness` |
| 140 | 1 | `crates/eaeh-runtime/tests/harness.rs` | `plugins_require_admin_enablement_grants_and_cannot_bypass_policy` | Plugins require admin enablement grants and cannot bypass policy | `cargo test -p eaeh-runtime --test harness` |
| 141 | 1 | `crates/eaeh-runtime/tests/harness.rs` | `supervision_escalates_stalled_work` | Supervision escalates stalled work | `cargo test -p eaeh-runtime --test harness` |
| 142 | 1 | `crates/eaeh-runtime/tests/harness.rs` | `config_rejects_unknown_fields_and_relative_programs` | Config rejects unknown fields and relative programs | `cargo test -p eaeh-runtime --test harness` |
| 143 | 1 | `crates/eaeh-runtime/tests/harness.rs` | `supervision_escalates_repeated_failures` | Supervision escalates repeated failures | `cargo test -p eaeh-runtime --test harness` |
| 144 | 2 | `crates/eaeh-runtime/tests/harness.rs` | `anthropic_provider_is_opt_in_and_needs_a_secret` | Anthropic provider is opt in and needs a secret | `cargo test -p eaeh-runtime --test harness` |
| 145 | 2 | `crates/eaeh-skills/tests/skills.rs` | `skills_require_human_approval_bound_to_digest` | Skills require human approval bound to digest | `cargo test -p eaeh-skills --test skills` |
| 146 | 2 | `crates/eaeh-skills/tests/skills.rs` | `rejected_skills_never_become_usable` | Rejected skills never become usable | `cargo test -p eaeh-skills --test skills` |
| 147 | 2 | `crates/eaeh-skills/tests/skills.rs` | `registration_validation` | Registration validation | `cargo test -p eaeh-skills --test skills` |
| 148 | 2 | `crates/eaeh-skills/tests/skills.rs` | `dependency_cycles_are_rejected` | Dependency cycles are rejected | `cargo test -p eaeh-skills --test skills` |
| 149 | 2 | `crates/eaeh-skills/tests/skills.rs` | `skills_never_extend_agent_permissions` | Skills never extend agent permissions | `cargo test -p eaeh-skills --test skills` |
| 150 | 2 | `crates/eaeh-skills/tests/skills.rs` | `selection_is_deterministic_with_dependencies_first_and_lifecycle` | Selection is deterministic with dependencies first and lifecycle | `cargo test -p eaeh-skills --test skills` |
| 151 | 1 | `crates/eaeh-store/src/audit.rs` | `chain_verifies_and_detects_modification` | Chain verifies and detects modification | `cargo test -p eaeh-store --lib` |
| 152 | 1 | `crates/eaeh-store/src/audit.rs` | `chain_detects_deletion` | Chain detects deletion | `cargo test -p eaeh-store --lib` |
| 153 | 1 | `crates/eaeh-store/src/audit.rs` | `concurrent_appends_remain_gapless` | Concurrent appends remain gapless | `cargo test -p eaeh-store --lib` |
| 154 | 1 | `crates/eaeh-store/src/audit.rs` | `details_are_redacted_before_storage` | Details are redacted before storage | `cargo test -p eaeh-store --lib` |
| 155 | 1 | `crates/eaeh-store/src/audit.rs` | `rolled_back_transaction_leaves_no_audit` | Rolled back transaction leaves no audit | `cargo test -p eaeh-store --lib` |
| 156 | 1 | `crates/eaeh-store/src/idempotency.rs` | `first_result_wins` | First result wins | `cargo test -p eaeh-store --lib` |
| 157 | 1 | `crates/eaeh-store/src/lib.rs` | `opens_and_migrates_idempotently` | Opens and migrates idempotently | `cargo test -p eaeh-store --lib` |
| 158 | 1 | `crates/eaeh-store/src/lib.rs` | `foreign_keys_are_enforced` | Foreign keys are enforced | `cargo test -p eaeh-store --lib` |
| 159 | 1 | `crates/eaeh-store/src/lib.rs` | `timestamps_roundtrip_and_sort` | Timestamps roundtrip and sort | `cargo test -p eaeh-store --lib` |
| 160 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `authorized_write_then_read_is_audited_and_evented` | Authorized write then read is audited and evented | `cargo test -p eaeh-tools --test gateway` |
| 161 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `autonomy_and_allowlist_enforced` | Autonomy and allowlist enforced | `cargo test -p eaeh-tools --test gateway` |
| 162 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `unregistered_or_contract_violating_calls_rejected` | Unregistered or contract violating calls rejected | `cargo test -p eaeh-tools --test gateway` |
| 163 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `isolation_requirements_refused_when_unavailable` | Isolation requirements refused when unavailable | `cargo test -p eaeh-tools --test gateway` |
| 164 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `external_api_denied_by_default_policy` | External api denied by default policy | `cargo test -p eaeh-tools --test gateway` |
| 165 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `high_risk_tool_requires_bound_human_approval` | High risk tool requires bound human approval | `cargo test -p eaeh-tools --test gateway` |
| 166 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `budgets_enforced_and_fail_closed` | Budgets enforced and fail closed | `cargo test -p eaeh-tools --test gateway` |
| 167 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `mandatory_controls_hold_when_hooks_missing_disabled_or_failing` | Mandatory controls hold when hooks missing disabled or failing | `cargo test -p eaeh-tools --test gateway` |
| 168 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `tool_output_secrets_are_redacted_and_untrusted` | Tool output secrets are redacted and untrusted | `cargo test -p eaeh-tools --test gateway` |
| 169 | 1 | `crates/eaeh-tools/tests/gateway.rs` | `cross_project_access_denied` | Cross project access denied | `cargo test -p eaeh-tools --test gateway` |
| 170 | 1 | `crates/eaeh-validation/src/lib.rs` | `gate_requires_deterministic_and_fails_closed` | Gate requires deterministic and fails closed | `cargo test -p eaeh-validation --lib` |
| 171 | 1 | `crates/eaeh-validation/src/lib.rs` | `semantic_findings_are_advisory_unless_opted_in` | Semantic findings are advisory unless opted in | `cargo test -p eaeh-validation --lib` |
| 172 | 1 | `crates/eaeh-validation/src/schema.rs` | `compiles_and_checks` | Compiles and checks | `cargo test -p eaeh-validation --lib` |
| 173 | 1 | `crates/eaeh-validation/src/schema.rs` | `remote_refs_are_not_fetched` | Remote refs are not fetched | `cargo test -p eaeh-validation --lib` |
| 174 | 3 | `crates/eaeh-workflow/tests/design_flow.rs` | `request_changes_revises_with_feedback_and_is_bounded` | Request changes revises with feedback and is bounded | `cargo test -p eaeh-workflow --test design_flow` |
| 175 | 3 | `crates/eaeh-workflow/tests/design_flow.rs` | `reject_is_final_and_gates_without_a_revision_transition_fall_back` | Reject is final and gates without a revision transition fall back | `cargo test -p eaeh-workflow --test design_flow` |
| 176 | 3 | `crates/eaeh-workflow/tests/design_flow.rs` | `bindings_resolve_outputs_inputs_and_artifacts` | Bindings resolve outputs inputs and artifacts | `cargo test -p eaeh-workflow --test design_flow` |
| 177 | 3 | `crates/eaeh-workflow/tests/design_flow.rs` | `quality_gate_fails_closed_before_anything_is_stored` | Quality gate fails closed before anything is stored | `cargo test -p eaeh-workflow --test design_flow` |
| 178 | 3 | `crates/eaeh-workflow/tests/design_flow.rs` | `bundles_bind_all_files_to_one_approval_and_trace_is_recorded_on_approval` | Bundles bind all files to one approval and trace is recorded on approval | `cargo test -p eaeh-workflow --test design_flow` |
| 179 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `definition_validation` | Definition validation | `cargo test -p eaeh-workflow --test engine` |
| 180 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `gate_blocks_until_human_approval_and_enforces_sod` | Gate blocks until human approval and enforces sod | `cargo test -p eaeh-workflow --test engine` |
| 181 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `rejection_returns_to_revision_with_history` | Rejection returns to revision with history | `cargo test -p eaeh-workflow --test engine` |
| 182 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `gate_cannot_be_bypassed_by_graph` | Gate cannot be bypassed by graph | `cargo test -p eaeh-workflow --test engine` |
| 183 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `retry_and_timeout` | Retry and timeout | `cargo test -p eaeh-workflow --test engine` |
| 184 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `cancellation_waiting_and_running` | Cancellation waiting and running | `cargo test -p eaeh-workflow --test engine` |
| 185 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `interrupted_workflow_recovers_without_duplicate_side_effects` | Interrupted workflow recovers without duplicate side effects | `cargo test -p eaeh-workflow --test engine` |
| 186 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `hybrid_orchestration_validates_planner_proposals` | Hybrid orchestration validates planner proposals | `cargo test -p eaeh-workflow --test engine` |
| 187 | 1 | `crates/eaeh-workflow/tests/engine.rs` | `start_requires_authorization_and_scope` | Start requires authorization and scope | `cargo test -p eaeh-workflow --test engine` |
| 188 | 1 | `crates/eaeh-workspace/src/artifacts.rs` | `versions_integrity_and_scope` | Versions integrity and scope | `cargo test -p eaeh-workspace --lib` |
| 189 | 1 | `crates/eaeh-workspace/src/lib.rs` | `relative_path_validation` | Relative path validation | `cargo test -p eaeh-workspace --lib` |
| 190 | 1 | `crates/eaeh-workspace/src/locks.rs` | `overlap_rules` | Overlap rules | `cargo test -p eaeh-workspace --lib` |
| 191 | 1 | `crates/eaeh-workspace/src/workspace.rs` | `write_requires_lock_and_read_back` | Write requires lock and read back | `cargo test -p eaeh-workspace --lib` |
| 192 | 1 | `crates/eaeh-workspace/src/workspace.rs` | `concurrent_holders_conflict` | Concurrent holders conflict | `cargo test -p eaeh-workspace --lib` |
| 193 | 1 | `crates/eaeh-workspace/src/workspace.rs` | `expired_locks_can_be_taken_over` | Expired locks can be taken over | `cargo test -p eaeh-workspace --lib` |
| 194 | 1 | `crates/eaeh-workspace/src/workspace.rs` | `traversal_and_symlink_escapes_rejected` | Traversal and symlink escapes rejected | `cargo test -p eaeh-workspace --lib` |
| 195 | 1 | `crates/eaeh-workspace/src/workspace.rs` | `scope_mismatch_and_cleanup` | Scope mismatch and cleanup | `cargo test -p eaeh-workspace --lib` |
| 196 | 3 | `tests/tests/design_demo.rs` | `brd_to_requirements_hld_and_lld_with_gates_revisions_and_traceability` | Brd to requirements hld and lld with gates revisions and traceability | `cargo test -p eaeh-acceptance --test design_demo` |
| 197 | 3 | `tests/tests/design_demo.rs` | `injected_brd_instructions_cannot_change_instructions_tools_or_approvals` | Injected brd instructions cannot change instructions tools or approvals | `cargo test -p eaeh-acceptance --test design_demo` |
| 198 | 3 | `tests/tests/design_demo.rs` | `brds_are_project_scoped_and_classification_bounded` | Brds are project scoped and classification bounded | `cargo test -p eaeh-acceptance --test design_demo` |
| 199 | 1 | `tests/tests/e2e.rs` | `agentic_workflow_with_planning_gate_and_full_traceability` | Agentic workflow with planning gate and full traceability | `cargo test -p eaeh-acceptance --test e2e` |
| 200 | 1 | `tests/tests/e2e.rs` | `agent_build_tool_runs_only_inside_container_isolation` | Agent build tool runs only inside container isolation | `cargo test -p eaeh-acceptance --test e2e` |
| 201 | 1 | `tests/tests/e2e.rs` | `secrets_never_reach_audit_events_or_agent_context` | Secrets never reach audit events or agent context | `cargo test -p eaeh-acceptance --test e2e` |
| 202 | 1 | `tests/tests/fitness.rs` | `crate_dependencies_follow_the_layering` | Crate dependencies follow the layering | `cargo test -p eaeh-acceptance --test fitness` |
| 203 | 1 | `tests/tests/fitness.rs` | `no_internal_dependency_cycles` | No internal dependency cycles | `cargo test -p eaeh-acceptance --test fitness` |
| 204 | 1 | `tests/tests/fitness.rs` | `governance_is_separated_from_intelligent_orchestration` | Governance is separated from intelligent orchestration | `cargo test -p eaeh-acceptance --test fitness` |
| 205 | 3 | `tests/tests/fitness.rs` | `design_models_are_deterministic_and_independent_of_intelligence` | Design models are deterministic and independent of intelligence | `cargo test -p eaeh-acceptance --test fitness` |
| 206 | 1 | `tests/tests/fitness.rs` | `testkit_is_never_a_production_dependency` | Testkit is never a production dependency | `cargo test -p eaeh-acceptance --test fitness` |
| 207 | 1 | `tests/tests/fitness.rs` | `only_store_uses_sqlx_directly` | Only store uses sqlx directly | `cargo test -p eaeh-acceptance --test fitness` |
| 208 | 1 | `tests/tests/fitness.rs` | `unsafe_code_is_confined_to_isolation` | Unsafe code is confined to isolation | `cargo test -p eaeh-acceptance --test fitness` |
| 209 | 1 | `tests/tests/fitness.rs` | `all_database_writes_use_the_serialized_writer` | All database writes use the serialized writer | `cargo test -p eaeh-acceptance --test fitness` |
| 210 | 1 | `tests/tests/fitness.rs` | `no_credentials_in_configuration_and_prompts_are_assets` | No credentials in configuration and prompts are assets | `cargo test -p eaeh-acceptance --test fitness` |
| 211 | 2 | `tests/tests/fitness.rs` | `http_client_and_provider_are_confined` | Http client and provider are confined | `cargo test -p eaeh-acceptance --test fitness` |
| 212 | 2 | `tests/tests/fitness.rs` | `governed_context_constructors_have_single_producers` | Governed context constructors have single producers | `cargo test -p eaeh-acceptance --test fitness` |
| 213 | 2 | `tests/tests/knowledge_demo.rs` | `agent_reviews_api_spec_with_approved_skill_and_enterprise_knowledge` | Agent reviews api spec with approved skill and enterprise knowledge | `cargo test -p eaeh-acceptance --test knowledge_demo` |
| 214 | 1 | `tests/tests/logging.rs` | `logs_are_redacted_and_carry_correlation_ids` | Logs are redacted and carry correlation ids | `cargo test -p eaeh-acceptance --test logging` |

## Not covered (explicit)

* Performance / load tests (Phase 5; single-writer SQLite throughput not measured).
* Agent evaluation against real models (Phase 5).
* **Live Anthropic API run** – not authorised (P2-D6); the adapter is verified against a local
  mock server only.
* Semantic retrieval quality (built-in embedder is non-semantic; A-29); retrieval at scale
  (brute-force vector scan; not load-tested).
* Whether cited text actually supports an agent's claim (only citation provenance is checked).
* Phase 3 generation with a real model (BRD → requirements → HLD → LLD): authorised (P3-D1) but not executed because no API key was supplied; generation quality is unverified (A-30).
* Independent evaluation of the requirements analyzer and of generated designs (labels by the implementer only; A-37, Phase 5).
* Multi-process concurrent writers (CLI + server on the same database) – design relies on
  `BEGIN IMMEDIATE` + optimistic versions; not stress-tested (A-20).
* Docker as container runtime (only Podman 5.5.1 exercised; ISO-L2).
* Linux host execution of the process-isolation suite (developed and verified on macOS).

