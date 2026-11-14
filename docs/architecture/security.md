# Security Architecture (Phases 1–3)

Threats, mitigations and residual risks: [security/THREAT-MODEL.md](security/THREAT-MODEL.md).

## Identity and authentication

* Principals: `human`, `service` (stored) and `agent` (ephemeral, created by the agent
  runtime). Agent principals have **no roles** (`has_role` is structurally false), belong to
  exactly one project, and carry their definition and autonomy level.
* API/CLI authentication: bearer tokens (ADR-0011). No default credentials; one-time
  bootstrap; hashed storage; expiry; revocation; atomic rotation; disabled principals
  rejected; uniform `unauthenticated` errors. Extension point for enterprise IdP: replace
  `IdentityService::authenticate` (A-14).

## Authorization (`RbacAuthorizer`)

Boundary check first (tenant equality; project access for scoped targets; agents cannot act
on platform/tenant targets), then capability check:

| Action | Admin | Operator | Approver | Viewer | Auditor | Knowledge steward | Agent |
|---|---|---|---|---|---|---|---|
| workflow.start / cancel | | ✔ | | | | | |
| workflow.read | ✔ | ✔ | ✔ | ✔ | ✔ | | |
| workflow.define, agent.register, plugin.manage, policy.manage, prompt.manage | ✔ | | | | | | |
| approval.read | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ | |
| **approval.decide** | | | ✔ (human only) | | | ✘ | ✘ never |
| tool.invoke | | ✔ | | | | | ✔ (+ allow-list + autonomy) |
| agent.delegate | | | | | | | ✔ (+ delegation policy) |
| principal.manage, token.manage_any | ✔ | | | | | | |
| token.manage_own | ✔ | ✔ | ✔ | ✔ | ✔ | ✔ | |
| audit.read | ✔ | | | | ✔ | | |
| artifact.read | ✔ | ✔ | ✔ | ✔ | | | ✔ |
| metrics.read | ✔ | ✔ | | | | | |
| **knowledge.manage**, **skill.manage** (Phase 2) | ✔ | | | | | ✔ | ✘ |
| knowledge.read, skill.read (Phase 2) | ✔ | ✔ | ✔ | ✔ | | ✔ | knowledge.read only (project-bound, via `knowledge.search`) |

Separation of duties: Admin does **not** include `approval.decide`; service principals can
never decide approvals; artifact authors (including agent principals) cannot approve their
own work. Phase 2 (ADR-0017): the `knowledge_steward` registers sources, ingests documents
and registers skills but cannot approve; the registrant/ingester of a standard or skill is
recorded as author and cannot approve it either.

## Approval gates

Enforced inside `ApprovalManager::decide` (not the UI or prompts): human, required role,
in scope, not an author, approval pending, artifact hash confirmed and equal. Recorded
atomically: approver, role, timestamp, decision, comments, artifact id/hash, workflow
instance, gate, correlation; audit record and event. Workflow states can require prior
gate approval independently of the transition graph. Tool calls whose policy effect is
`require_approval` need an approval bound to the exact tool version and input fingerprint.
Verified by engine, gateway, API and CLI tests (see TEST-STRATEGY).

Phase 3:
- **`request_changes`:** sends work back for revision. It is accepted only for workflow gates and only with comments.
- **Same checks as approve:** human approver, required role, not an author, matching artifact hash.
- **Bounded:** limited per gate by `max_revisions`.
- **Bundles:** HLD and LLD gates bind to the hash of a bundle manifest that itself binds every file hash (document, diagrams, ADRs). Approving it approves exactly those bytes.
- **Traceability:** links are recorded only after approval and carry the approval id.

Phase 2 subjects: `knowledge_document` (standards; bound to the document content hash) and
`skill` (bound to the canonical definition digest). A standard becomes `active` and a skill
`approved` only through `ApprovalManager` — the knowledge and skill services apply the
decision, they never decide it.

## Policy

ADR-0006: default deny, deny-overrides, enterprise rules cannot be weakened by project
rules, decisions list matched rules. Only the structural baseline ships.

## Knowledge scopes, classification and egress (Phase 2)

* Knowledge visibility (ADR-0016) is filtered **in SQL** on tenant, `active` status,
  classification ceiling and scope (enterprise, project's domain, project, caller's teams,
  workflow). Agents get project-bound contexts with ceiling `agent_max_classification` and
  no team scope.
* Classifications: `public < internal < confidential < restricted` (default `internal`).
  Every context item carries one; the assembled context's maximum is passed to the
  `model.invoke` policy with the provider's egress (`external`/`local`). Baseline rule
  `model-restricted-egress-deny`: restricted content never goes to an external provider;
  each denied candidate is audited; if all are denied the call fails with `policy_denied`.
* Only `eaeh-knowledge` can create enterprise-standard instructions and only `eaeh-skills`
  skill instructions (fitness-enforced); everything retrieved is untrusted, fenced data with
  a citation. Ingestion quarantines documents failing validation (e.g. embedded
  credentials) **without storing their content**. Deleting a document purges content,
  chunk text, FTS rows and vectors; the audit trail stays.
* FTS5 query text is reduced to quoted terms before `MATCH` (no query-syntax injection).
* Phase 3:
  - BRDs are project-scoped knowledge documents, `confidential` by default (`eaeh brd ingest`).
  - `knowledge.read` and `brd.sections` apply the same visibility predicate. An invisible document is reported as `not_found`, so other projects learn nothing about its existence.
  - BRD text reaches agents only as fenced, untrusted tool output.
  - Generated documents are rendered by the platform, which entity-encodes model text in Mermaid labels and escapes HTML in Markdown.

## Tenant and project isolation

Every scoped table carries `tenant_id` + `project_id`; store queries filter on both; the
API derives the tenant from the authenticated principal (cross-tenant requests cannot be
expressed); authorization checks project access; context assembly drops items from other
projects; workspaces and artifacts are opened only with a matching scope; agent principals
are bound to one project.

## Secrets and sensitive data

* `SecretString` never formats or serializes its value; `SecretsProvider` abstraction
  (`EnvSecretsProvider` reads only `EAEH_SECRET_*`); `Harness::secret()` registers fetched
  secrets with the shared `Redactor`.
* Redaction applied to: logs (redacting writer), audit details (before hashing), tool
  output (interceptor), context data, model responses, task errors, event delivery errors.
  Pattern redaction for `eaeh_…` tokens and `Bearer …` values; sensitive JSON keys.
* Validation: `secret.leak` deterministic validator for quality gates.
* Storage: database `0600`, data directory `0700`, token hashes only, workspaces `0700`.
* Model provider credentials (Phase 2, ADR-0015): only from `EAEH_SECRET_ANTHROPIC_API_KEY`
  through the secrets provider, registered with the redactor, sent only in the `x-api-key`
  header; never in configuration (fitness test checks config keys and `sk-ant-` literals),
  logs, audit, context or events. The provider is disabled unless `providers.anthropic.enabled
  = true`; enabling it without the secret fails start-up.
* External AI execution (ADR-0025):
  * EAEH never receives the AI client's credentials or model session. The MCP server runs as the developer's stored EAEH identity, and generated client files contain no tokens (tested).
  * Hand-off packages are assembled by the governed context path (redaction, fencing, classification). The `model.invoke` egress policy applies to the `external-ai-client`, so restricted content is never handed off.
  * Packages hold project data. `task show --out` writes them as 0600 files, and only operators (`task.execute_external`) can read them.
  * Client tool calls run as the **agent** principal through the tool gateway (allow-list, autonomy, budgets, workspace confinement, audit), never as the developer.
  * Submissions are untrusted model output: schema, skill contracts, citations, domain validators, quality gates and file-existence checks apply.
  * The submitter becomes a co-author and cannot approve the work.
  * No fallback: no provider is registered in `external_ai` mode, and other modes refuse handed-off tasks.
* Data classification labels exist for knowledge and context (Phase 2). Not yet: encryption
  at rest, KMS/vault provider, retention/deletion jobs, residency enforcement (A-15).

## Prompt and agent security

Untrusted content is typed (`Untrusted<T>`) and can only enter context as fenced data;
model output never authorizes anything — every proposed tool call goes through the gateway;
agents cannot approve; delegation is bounded; loop detection and budgets stop runaway agents.

## Developer experience (ADR-0024)

* CLI identities are stored per profile in `$EAEH_HOME/credentials.json`. The directory is 0700 and the file 0600; writes are atomic, and EAEH refuses to use the store if it is group/world readable. This is plaintext, like `gh`/`kubectl` credentials; there is no OS keychain yet. Profiles are bound to the data directory of their installation.
* Tokens are never printed by default: `token issue/rotate` need `--token-file`, `--store-profile` or an explicit `--print`, and `auth status` shows metadata only.
* Identity selection is automatic only when exactly one stored identity holds the needed permission. **Approval decisions always need an explicit approver identity.**
* `eaeh project init` uses only existing permissions (`principal.manage`, `knowledge.manage`, `skill.manage`):
  - it creates principals with a single role;
  - `grant_project_access` adds project access to an existing principal, is audited, never changes roles, and does not cross tenants;
  - administrators still cannot start workflows or approve.
* Approvers see pending standards through `KnowledgeService::document_for_approval` (`approval.read` on the approval scope), not `knowledge.manage`.
* The MCP server runs as the developer profile through the same services. It has no approval tool. Governed activities start only when readiness passes.
* `CLAUDE.md`, `.mcp.json` and `.eaeh/project.toml` contain no credentials (tested).
* The deterministic replay provider (ADR-0023) is local, off by default, flagged by readiness and identifiable in provenance.

## Supply chain

`Cargo.lock` + `--locked`; `cargo-deny` (advisories, licences, bans, sources) in CI; one
advisory exception pending approval (A-10, ADR-0003). Phase 2 added `reqwest`/`rustls` (ring
provider, no native-tls/OpenSSL), confined to `eaeh-provider-anthropic` by a fitness test,
and allowed the `CDLA-Permissive-2.0` licence (Mozilla CA root data via `webpki-roots`). SBOM generation is not yet automated
(A-16).
