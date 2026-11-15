# Approved design artifacts in the project workspace – investigation and plan

| Field | Value |
|---|---|
| Status | IMPLEMENTED 2026-11-15 – M1–M4 as recommended; see docs/implementation/EAEH-ARTIFACT-MATERIALIZATION.md and ADR-0027 |
| Date | 2026-11-15 |
| Scope | Supplementary fix within Phases 1–3. It covers Requirements, HLD and LLD only. Delivery source bundles (the early Phase 4 part, ADR-0025) keep the explicit `eaeh artifact export`. |
| Request | "EAEH HLD/LLD Artifact Persistence and Project Workspace Synchronization" (pasted request; its text ends partway through §11 *Testing*) |

## 1. Finding: where the documents stop

The evidence below comes from source code and the user's real run `01a1db3c-d116-7109-ab29-4dfdc856cf4d`.

| Step | Implementation | Works? | Evidence |
|---|---|---|---|
| BRD ingestion | `brd` tool step (`brd.sections`) reads the manifest's `[brd]` document | yes | transition `brd → analyse` |
| Requirements / HLD / LLD generation | `agent_task` states handed to the AI client (`external_ai`), validated by quality gates | yes | about 2 minutes per stage; quality gates `requirements/hld/lld: passed (0 blocking)` |
| Persistence and versioning | `ArtifactStore::store` (`crates/eaeh-workspace/src/artifacts.rs`). The content-addressed blob is written to `<data_dir>/artifacts/<tenant>/sha256/..` (atomic tmp + rename). A row is added to `artifacts` with a unique `(tenant, project, name, version)`. The store also writes `artifact.create` to the audit log and emits an `ArtifactGenerated` event. | yes | seven artifacts, version 2 each, in `.eaeh-data` |
| Rendering | `render_*` tool steps store a **bundle manifest**: `{files: {path → {artifact_id, hash}}}` covering `hld.md`, `hld.json`, `diagrams/*.mmd` and `adr/*.md` | yes | export hash check passed for every file |
| Approval | `approval_gate` binds to the bundle hash (`artifact_from`). `check_gate` and the completion guard verify approver, role, author and hash. | yes | `approve_hld` 18:32:57 and `approve_lld` 19:15:14 by `approver` |
| **Materialization into the project** | **None.** No workflow state, hook or post-approval step writes to the application repository. The only path is the manual `eaeh artifact export <id> --out <dir>` (`crates/eaeh-cli/src/main.rs:34`). | **no** | the project folder has no design files; no audit action writes there |
| Workspace mapping | The repository root is known only from the repo-local manifest `.eaeh/project.toml` (`[workspace] root`). The `projects` table has no root. The `workspaces` table holds EAEH-managed isolated workspaces only. | partial | `migrations/0001_initial.sql` |

**Root cause.** This is not a persistence, path, permission or transaction fault. EAEH was built with a *store-and-export* contract. Approval completes the governed record, but nothing materializes it:
* no post-approval step exists;
* the engine and the processes that resume it (the portal, `eaeh serve`, the MCP server) cannot resolve the project's repository root from authoritative state, because only the CLI reads `.eaeh/project.toml` from the current directory.

The status text points to `eaeh artifact export`, but nothing tells the developer that this step is required. Nothing reports that the documents are missing either.

**Related gap.** `requires_approval_of` (in `run_work`, `engine.rs`) checks the gate's status, subject and scope, but **not** that the approved hash is still the current artifact version. In normal transitions this cannot diverge. The completion guard's `check_gate` is stricter, and the plan applies it here as well, so an LLD can never start from an outdated HLD approval.

## 2. Storage contract (proposed)

* **Authoritative:** the EAEH artifact store, with these properties:
  * content-addressed, immutable versions;
  * provenance, quality gates, approval records and audit.
  * History is never overwritten there: every revision is a new version, and rejected or change-requested versions remain.
* **Materialized copy:** only the **approved** version of each design bundle, written into the registered project workspace. It is a reproducible projection: each file must equal the stored file hash. Drafts are not written to the workspace. They are reviewed in the portal, `eaeh approval review` or MCP `eaeh_artifact_read`.

## 3. Decisions for approval

| # | Decision | Recommendation | Alternatives |
|---|---|---|---|
| M1 | When and how materialization runs, and what happens on failure | **Engine-driven, declared on the approval gate** (`"materialize": "docs/architecture/hld"`). After an approval is recorded, the engine writes the bundle **before** taking the `approved` transition. If the write fails, the workflow **does not advance**. It stays at the gate as `blocked` with the reason, the failure is audited, and `eaeh artifact sync` retries it and then resumes. | (a) Separate `tool_step` states after each gate. They are visible in the graph, but a failure would end the run as `failed`. (b) Keep manual export and only report the status. |
| M2 | Folder layout | **One folder per bundle** under `docs/architecture/`: `requirements/`, `hld/` and `lld/`, each holding its `*.md`, `*.json`, `diagrams/` and `adr/`. The BRD stays where it is (manifest `[brd] path`); it is not copied. | The flat `docs/architecture/HLD.md` and `LLD.md` from the request. Diagrams, ADRs and JSON models would then collide or be dropped. |
| M3 | Overwrite safety | **Write a file only if it is absent, or still byte-identical to the version EAEH last materialized there** (tracked by hash). Files changed locally or not created by EAEH are reported as conflicts, and nothing is written for that bundle. `eaeh artifact sync --replace-modified` is an explicit human override. It is audited. | Always overwrite; or never overwrite once a file exists. |
| M4 | Provenance in the repository | **`.eaeh/artifacts.json`** in the existing manifest folder, committed with Git. For each document it records: project, workflow, artifact id, version, hash, source artifacts, approval id, approver, approval time, materialization time and files with their hashes. The documents themselves stay byte-identical to the approved artifacts. | A front-matter header in each `.md` file. This changes the bytes, so the copy no longer equals the approved hash. |

The following follow from M1–M4 and are not separate questions:
* **Workspace registration.** Migration `0006` adds:
  * `project_workspaces (project_id → root_path, registered_by, at)`. `eaeh project init` writes it, and is idempotent, so an existing project is registered by re-running it. So is `eaeh artifact sync` when run inside the repo, if the manifest's project id matches.
  * `artifact_materializations`: artifact, path, hash, status, error, workflow, approval, time.
  * Writes are confined to the registered root: relative paths only, no `..`, no symlinks on the path, atomic tmp + rename, and every file is re-read and hash-verified after writing.
* **CLI.**
  * `eaeh artifact status`: each design artifact's version, hash, approval, workspace path, materialization status and reason.
  * `eaeh artifact sync [--workflow <id>] [--replace-modified]`: reconciliation, idempotent and audited.
  * Existing `eaeh workflow status` and `eaeh artifact export` stay, extended with materialization lines.
* **MCP.** `eaeh_workflow_status` includes the materialization status (read-only). Clients get no write capability.
* **Portal.** The workflow detail and document pages show, per artifact: name, type, version, generation, validation and approval status, workspace path, materialization status and last verification time. Status shows **verified** only if the file on disk is re-hashed and matches *at read time*. The other states are `not materialized` (with the reason), `modified locally`, `missing` and `failed`.
* **Events and audit.**
  * `artifact.materialize` audit entries, outcome succeeded, denied (conflict or outside the workspace) or failed;
  * an `ArtifactMaterialized` event;
  * `BeforeFileModification` and `AfterFileModification` hooks (§6) dispatched for every file. Hooks cannot replace the confinement checks.
* **Traceability.**
  * HLD → approved Requirements and LLD → approved HLD are recorded as the source artifacts in `.eaeh/artifacts.json` and the portal.
  * The `requires_approval_of` check becomes hash-bound (see §1).
  * No new trace graph is added: the existing `eaeh trace` links stay.

## 4. Tests (planned)
* **Persistence:** requirements, HLD and LLD versions, content and metadata retained; hashes correct.
* **Materialization:**
  * resolves the correct root and layout;
  * the bytes written equal the stored hashes;
  * idempotent re-run;
  * a new approved version replaces the previous materialized one;
  * conflict on a locally modified file;
  * a path-traversal or symlink-escape bundle is refused;
  * an unregistered workspace blocks the gate, and `sync` recovers it and the workflow resumes;
  * draft versions are never written;
  * the audit and `.eaeh/artifacts.json` are correct.
* **Gate:** an LLD with an outdated HLD approval hash is refused.
* **CLI, MCP and API/portal:** each reports the status; the portal shows `verified` only after the re-hash.
* **Real data:** a deterministic `simple-springboot-api` run through all three gates. The approvals in the run are decided by the test fixture's approver identity in automated tests only. In acceptance runs the user decides.
* **Existing suites stay green:** `scripts/ci.sh`, vitest and Playwright.

## 5. Out of scope
* Phase 4–8 functionality.
* Automatic materialization of delivery source bundles.
* Git commits by EAEH: the developer commits the documents.
* Remote or multi-machine workspaces. Materialization requires the process that resumes the workflow to see the registered root; otherwise the gate blocks with that reason, and `sync` from the repo resolves it.
