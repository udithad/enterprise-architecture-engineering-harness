# EAEH Threat Model (Phases 1–3)

Method: trust boundaries × STRIDE-style threats. Evidence refers to automated tests
(crate/test-file::test-name) executed in `docs/implementation/evidence/phase-01-ci.log`
(T1–T27), `docs/implementation/evidence/phase-02-ci.log` (T28–T40) and `docs/implementation/evidence/phase-03-ci.log` (all, including T41–T50).

## Trust boundaries

1. API/CLI caller → harness
2. Model output → runtime
3. Retrieved content / tool output → agent context
4. Tool execution → host
5. Plugin code → core
6. Workspace content → host filesystem
7. Secrets → logs / audit / context / events
8. Agent → agent (delegation)
9. Data directory (database, WAL, blobs) → process
10. Dependencies (supply chain) → build
11. Knowledge document author / steward → knowledge store (Phase 2)
12. Harness → external LLM provider (data egress, Phase 2)
13. Skill author → agent instructions (Phase 2)
14. BRD author → requirements/design agents (Phase 3)
15. Generated design (model text) → rendered documents and diagrams viewed by reviewers (Phase 3)

## Threats, mitigations, evidence, residual risk

| # | Boundary | Threat | Mitigation | Evidence | Residual risk |
|---|---|---|---|---|---|
| T1 | 1 | Unauthenticated access | Bearer token on all routes except public health | `eaeh-api/tests/api.rs::authentication_and_error_contract` | Low |
| T2 | 1 | Stolen / long-lived token | Hash-only storage, expiry, revocation, rotation, redaction | `eaeh-governance identity::tests::*`, `api.rs::pagination_tokens_and_rate_limits` | Medium: no IP binding / MFA (A-14) |
| T3 | 1 | Default or hard-coded credentials | None exist; one-time bootstrap; fitness scan for token literals | `cli.rs::cli_end_to_end`, `fitness.rs::no_credentials_in_configuration_and_prompts_are_assets` | Low |
| T4 | 1 | Approval bypass via direct API call | Decision enforced in `ApprovalManager`; engine gate prerequisite check | `api.rs::approval_gate_cannot_be_bypassed_through_the_api`, `engine.rs::gate_cannot_be_bypassed_by_graph` | Low |
| T5 | 1/2 | Self-approval by agents or authors | Agents role-less; authors excluded; humans only | `approval::tests::self_approval_and_non_humans_rejected`, `engine.rs::gate_blocks_until_human_approval_and_enforces_sod` | Low |
| T6 | 1 | Approving a changed artifact | Approval bound to artifact SHA-256 | `approval::tests::artifact_hash_must_match` | Low |
| T7 | 1 | Cross-tenant / cross-project access | Tenant from principal, scope checks, scoped queries | `api.rs::cross_project_isolation_and_role_checks`, `gateway.rs::cross_project_access_denied`, `authz::tests::cross_tenant_and_cross_project_denied` | Low |
| T8 | 1 | Request flooding / oversized bodies | Per-principal rate limit; body limit | `api.rs::pagination_tokens_and_rate_limits`, `authentication_and_error_contract` | Medium: in-memory limiter per process; no unauthenticated-flood protection beyond token check (A-17) |
| T9 | 2 | Prompt injection authorising actions | Model output is a proposal; gateway authz/autonomy/policy; agents cannot approve | `agents.rs::prompt_injection_in_tool_output_cannot_escalate` | Low for authorization; model may still produce wrong content (Phase 5 evaluation) |
| T10 | 3 | Indirect injection overriding instructions | Typed trust levels, fenced data blocks, fixed notice | `context::tests::untrusted_content_cannot_alter_instructions` | Medium: models can still be influenced within their allowed tools |
| T11 | 3 | Cross-project context leakage | Scope filtering in assembler | `context::tests::scope_expiry_conflicts_budget_and_redaction` | Low |
| T12 | 4 | Network exfiltration from executed code | Container `--network none`; executing tools require it | `container_isolation.rs::network_access_is_denied`, `gateway.rs::isolation_requirements_refused_when_unavailable` | Low with container runtime |
| T13 | 4 | Host filesystem access/modification | Read-only rootfs, workspace-only mount | `container_isolation.rs::filesystem_is_confined_to_workspace` | Low |
| T14 | 4 | Resource exhaustion (fork bomb, memory, CPU, runaway) | pids/memory/cpu limits, timeouts with kill | `memory_limit_is_enforced`, `process_count_limit_is_enforced`, `timeout_kills_and_removes_container`, `cpu_time_limit_terminates_busy_loop` | Low |
| T15 | 4 | Privilege escalation in sandbox | Non-root, all caps dropped, no-new-privileges | `runs_as_unprivileged_user_without_capabilities` | Low; kernel exploits out of scope (microVM in Phase 8) |
| T16 | 4 | Silent isolation downgrade | Declared capabilities; refusal | `selection_refuses_instead_of_downgrading`, `refuses_requirements_it_cannot_enforce` | Low |
| T17 | 5 | Malicious/tampered plugin manifest | Digest + ed25519 with trusted keys; admin enablement; grants | `plugin::tests::signature_verification`, `harness.rs::plugins_require_admin_enablement_grants_and_cannot_bypass_policy` | **Medium/accepted by design**: enabled in-process plugins are trusted code with process privileges (clarification 1) |
| T18 | 5 | Plugin bypassing policy via contributed tools | Tools only via gateway | same as T17 | Low |
| T19 | 6 | Path traversal / symlink escape | Component validation, symlink checks, `O_NOFOLLOW` | `workspace::tests::traversal_and_symlink_escapes_rejected` | Low; TOCTOU between check and rename mitigated by locks (residual Low) |
| T20 | 6 | Concurrent conflicting modifications | Path locks with TTL | `workspace::tests::concurrent_holders_conflict` | Low |
| T21 | 7 | Secrets in logs/audit/context/events | Redactor everywhere; `SecretString` | `e2e.rs::secrets_never_reach_audit_events_or_agent_context`, `logging.rs::logs_are_redacted_and_carry_correlation_ids`, `observability::tests::log_lines_are_redacted`, `audit::tests::details_are_redacted_before_storage` | Medium: secrets not registered and not matching patterns can leak if a tool returns them (A-15) |
| T22 | 8 | Uncontrolled delegation / recursion / spawn storms | Allow-list, depth, cycle, fan-out, duplicate, concurrency budget | `agents.rs::delegation_controls`, `depth_limit_enforced` | Low |
| T23 | 9 | Audit tampering | SHA-256 hash chain, gapless sequence, verification command | `audit::tests::chain_verifies_and_detects_modification`, `chain_detects_deletion` | Medium: an attacker with DB write access can rewrite the whole chain consistently (no external anchoring; A-18) |
| T24 | 9 | Crafted DB/WAL pair (RUSTSEC-2026-0459) | `0700` data dir, `0600` DB | `eaeh-store tests::opens_and_migrates_idempotently` | **Medium – exception pending approval (A-10)** |
| T25 | 10 | Vulnerable / incompatible-licence dependencies | `cargo-deny` gate, lockfile | `phase-01-ci.log` (cargo-deny section) | Low except T24 |
| T26 | 2 | Runaway cost / loops | Budgets (model calls, tokens, cost, steps, corrections), loop detection, circuit breaker | `model_client.rs::all_failing_and_budget_limits`, `agents.rs::bounded_correction_loop_detection_and_step_budget` | Low |
| T27 | 1 | Information disclosure through errors | Internal errors masked; not-found for unauthorized reads | `api.rs::cross_project_isolation_and_role_checks` | Low |
| T28 | 11/3 | Indirect prompt injection via knowledge documents | Non-standard knowledge enters only as untrusted, fenced, cited data; standards require hash-bound human approval | `knowledge_agents.rs::injected_knowledge_cannot_override_standards_or_instructions` | Medium: the model may still be influenced in its content (Phase 5 evaluation) |
| T29 | 11 | Unapproved/altered standard becoming an instruction | `pending_approval` until approver decides on the exact content hash; SoD (steward cannot approve) | `knowledge.rs::standards_activate_only_after_human_approval_bound_to_hash`, `::rejected_standards_never_activate`, `api.rs::knowledge_and_skill_routes_are_governed` | Low |
| T30 | 11 | Credentials ingested into the knowledge base | Validation quarantines; quarantined content not stored, not indexed | `knowledge.rs::documents_with_credentials_are_quarantined_and_never_retrievable` | Medium: only pattern-detectable secrets are caught |
| T31 | 3 | Cross-tenant / cross-project / out-of-scope knowledge leakage | SQL visibility filter (tenant, scope, status, ceiling); agent contexts project-bound | `knowledge.rs::scope_tenant_and_classification_isolation`, `::agent_retrieval_is_project_bound_and_ceiling_limited` | Low |
| T32 | 11 | FTS5 query-syntax injection | Query reduced to quoted terms | `search::tests::fts_query_is_neutralised`, `knowledge.rs::fts_injection_cannot_break_queries_or_scope` | Low |
| T33 | 12 | Restricted data sent to an external LLM | `model.invoke` egress policy on max context classification; denial audited | `policy::tests::restricted_content_never_egresses_to_external_models`, `model_client.rs::restricted_content_is_not_sent_to_external_providers`, `knowledge_agents.rs::restricted_knowledge_is_never_sent_to_external_models` | Medium: depends on correct classification at ingestion; task input defaults to `internal` |
| T34 | 12 | Provider API key disclosure | Secrets provider only; redactor; header only; config fitness check | `mock_api.rs::error_mapping_and_no_credential_leakage`, `harness.rs::anthropic_provider_is_opt_in_and_needs_a_secret`, `fitness.rs::no_credentials_in_configuration_and_prompts_are_assets` | Low |
| T35 | 12 | Unintended egress by default | Provider disabled unless enabled; HTTP client confined to one crate | `harness.rs::anthropic_provider_is_opt_in_and_needs_a_secret`, `fitness.rs::http_client_and_provider_are_confined` | Low |
| T36 | 13 | Skill escalating agent permissions | Binding requires skill tools ⊆ agent allow-list and skill risk ≤ agent autonomy; skills never grant | `skills.rs::skills_never_extend_agent_permissions` | Low |
| T37 | 13 | Unapproved / tampered skill used | Approval bound to definition digest; only `approved` skills resolve; rejected never usable | `skills.rs::skills_require_human_approval_bound_to_digest`, `::rejected_skills_never_become_usable`, `knowledge_agents.rs::skills_require_a_configured_registry_and_must_be_approved` | Low |
| T38 | 13 | Skill instructions forged by other code | Single producer of skill/standard instructions | `fitness.rs::governed_context_constructors_have_single_producers` | Low |
| T39 | 2 | Fabricated citations presented as grounded | Citation validator: cited ids must have been retrieved in this run | `knowledge_agents.rs::fabricated_citations_fail_validation` | Medium: support of the claim by the cited text is not checked (Phase 5) |
| T40 | 11 | Stale or conflicting guidance | Freshness flags/exclusion; subject-key conflict detection with scope precedence; mandatory standards always included | `knowledge.rs::conflicts_freshness_and_standard_precedence` | Medium: semantic contradictions are not detected |
| T41 | 14 | Instructions injected in a BRD steer agents, tools or approvals | BRD only as fenced untrusted tool output; allow-list; agents cannot approve; deterministic gates | `design_demo.rs::injected_brd_instructions_cannot_change_instructions_tools_or_approvals` | Medium: content of drafts can still be influenced – humans review every stage |
| T42 | 14 | BRD of one project read by another project's agent/user | Visibility predicate in `read_document`; `not_found` outside scope | `design_demo.rs::brds_are_project_scoped_and_classification_bounded` | Low |
| T43 | 2 | Model fabricates requirements without BRD basis | Sources must be retrieved chunk citations (citation validator); BRD coverage; human approval | `requirements.rs::structural_defects_are_blocking`, `knowledge_agents.rs::fabricated_citations_fail_validation` | Medium: a cited section may not actually support the requirement (A-36) |
| T44 | 15 | Injection through rendered Mermaid/Markdown (directives, click handlers, HTML) | Platform rendering with entity encoding and identifier sanitising | `mermaid::tests::labels_and_identifiers_cannot_break_out`, `architecture.rs::model_text_cannot_inject_into_diagrams_or_documents` | Low |
| T45 | 1 | Approving a design whose diagrams/ADRs differ from what was reviewed | Gate bound to bundle manifest hash covering every file hash | `design_flow.rs::bundles_bind_all_files_to_one_approval_and_trace_is_recorded_on_approval` | Low |
| T46 | 2 | Design ignores mandatory standards | Compliance entry required per supplied mandatory standard; deviation needs ADR | `architecture.rs::mandatory_standards_must_be_addressed_and_deviations_need_an_adr` | Low for presence; correctness of the claim needs human review |
| T47 | 1 | Endless revise loops / reviewer fatigue | `max_revisions` per gate; audited | `design_flow.rs::request_changes_revises_with_feedback_and_is_bounded` | Low |
| T48 | 1 | request_changes misused on knowledge/skill approvals or without explanation | Only workflow gates; comments mandatory; same SoD checks | `approval::tests::request_changes_is_limited_to_workflow_gates_and_needs_comments`, `api.rs::request_changes_and_traceability_routes_are_governed` | Low |
| T49 | 9 | Traceability forged or polluted by unapproved drafts | Links computed by the platform from validated models; recorded only on approval with approval id | `design_flow.rs::bundles_bind_all_files_to_one_approval_and_trace_is_recorded_on_approval`, `design_demo.rs::brd_to_requirements_hld_and_lld_with_gates_revisions_and_traceability` | Low |
| T50 | 4 | Quality gate bypass (missing validator, unknown gate, gate not run) | Unknown gate rejected at registration; missing validator/engine fails closed; gate before storage | `design_flow.rs::quality_gate_fails_closed_before_anything_is_stored`, `::bindings_resolve_outputs_inputs_and_artifacts`, `requirements.rs::validator_runs_in_a_fail_closed_quality_gate` | Low |
