# Approved design documents in the project workspace (supplementary enhancement)

> **Supplementary document.** This is a fix within Phases 1–3. It does **not** change:
> * the eight-phase roadmap;
> * any phase definition, dependency, acceptance criterion or next step in `claude-instructions.md` or `docs/implementation/ROADMAP.md`;
> * any existing architectural decision.
>
> | | |
> |---|---|
> | Plan and approved decisions M1–M4 | `docs/implementation/stages/ARTIFACT-MATERIALIZATION-PLAN.md` |
> | Decision record | [ADR-0027](../architecture/adr/ADR-0027-approved-artifact-materialization.md) |
> | Request | "EAEH HLD/LLD Artifact Persistence and Project Workspace Synchronization". The pasted request text ends partway through its §11 (*Testing*); the visible requirements are implemented. |

## 1. The defect and its cause

Approved Requirements, HLD and LLD documents never appeared in the application repository.

Generation, validation, persistence, versioning and approval all worked; the user's run `01a1db3c…` shows real external-AI stages, quality gates passed, and three hash-bound human approvals. The fault was a missing link in the lifecycle:

* **Store-and-export only.** The engine stored every output in the content-addressed artifact store (`<data_dir>/artifacts/…` plus the `artifacts` table). No step wrote an approved document into the repository. Only a manual `eaeh artifact export <id> --out <dir>` did that, and no message said this step was required.
* **No authoritative workspace mapping.** The repository root existed only in the repo-local `.eaeh/project.toml`, which only the CLI reads from its current directory. The processes that resume a workflow after an approval could not know where the repository is:
  * the portal;
  * `eaeh serve`;
  * the MCP server;
  * the worker.

Not the cause: database-only persistence of content (the content is complete and hash-verified), path or permission errors, transaction failures, stage ordering, external AI submissions, versioning or overwrites.

## 2. Storage contract

| | EAEH artifact store (authoritative) | Project workspace (materialized copy) |
|---|---|---|
| What | Every version of every artifact: content, metadata, id, project, workflow, stage, version, SHA-256, provenance, quality gates. Approval records and audit sit alongside it. | The **current approved** version of each design document, as plain files for VS Code, Git and later development work. |
| History | Immutable. A new version is a new record, and rejected or change-requested versions are kept. | One version per folder. A newer approved version replaces it, and the history stays in the store. |
| Integrity | Hash-checked on every read. | Each file is byte-identical to a stored file artifact. It is re-hashed after writing and on every status check. |
| Drafts | Yes. They are reviewed in the portal, with `eaeh review` / `eaeh approval review`, and through MCP `eaeh_artifact_read`. | **Never.** Only approved versions are written there. `eaeh review` still exports review copies of drafts to `review/<gate>/`. That folder holds unapproved scratch copies; keep it out of Git. |

## 3. Folder layout (decision M2)

```text
<project repository>/
├── BRD-….md                          # the BRD, at the path in .eaeh/project.toml [brd]; not copied
├── .eaeh/
│   ├── project.toml                  # manifest (existing)
│   └── artifacts.json                # provenance index of materialized documents (new, M4)
└── docs/architecture/
    ├── requirements/                 # approve_requirements
    │   ├── requirements.md  requirements.json  analysis.json
    ├── hld/                          # approve_hld
    │   ├── hld.md  hld.json  diagrams/*.mmd  adr/d-00N-*.md
    └── lld/                          # approve_lld
        ├── lld.md  lld.json  diagrams/*.mmd
```

The folder is declared on each approval gate of the workflow definition (`"materialize": "docs/architecture/hld"`). Both shipped definitions declare it, and their version is bumped:
* `engineering.design` 1.1.0;
* `requirements.analysis` 1.1.0.

Definition validation rejects:
* absolute paths;
* `.` and `..` components and empty components (`//`);
* `.git` and `.eaeh`.

Runs started under 1.0.0 have no `materialize` field. For those, `eaeh artifact sync` uses the same gate in the latest definition version.

## 4. Lifecycle (decision M1)

```
agent task ─▶ quality gate ─▶ ArtifactStore (version N, hash)        ← persisted before approval
render step ─▶ bundle manifest {path → file artifact + hash}         ← what the approver reviews
approval gate ─▶ approval request bound to the bundle hash
   human decision (portal / CLI) ─▶ ApprovalManager (role, SoD, hash)
   engine, Approved:
      1. materialize gate.materialize  (Materializer, below)
         ├─ ok   ─▶ record trace links ─▶ `approved` transition ─▶ next stage
         └─ fail ─▶ instance stays `running` at the gate, data.blocked = {gate, reason, since},
                    audit `workflow.blocked`; every later drive (restart recovery,
                    `eaeh workflow resume`, `eaeh artifact sync`) retries idempotently
   changes_requested / rejected ─▶ nothing is written; history is kept in the store
next stage `requires_approval_of` ─▶ now hash-bound: the approval must be valid for the
      *current* version of the reviewed artifact, else the run fails before generating
      (no LLD from an outdated HLD approval)
```

* **Blocked is shown, not hidden.** The database status stays `running`, so no migration rebuilds the workflow tables. Every read model adds `blocked`, and the CLI and portal show **BLOCKED** with the reason.
* **Where the write happens.** It runs in whichever process resumes the engine: the portal, `eaeh serve`, the CLI or the worker. That process must be able to see the registered root. If it cannot, the gate blocks with *"registered workspace … is not accessible from this process"*.

## 5. Materializer (`crates/eaeh-workspace/src/materialize.rs`)

Each attempt runs these steps:
1. Resolve the registered root (`project_workspaces`). It must exist and be a directory.
2. Read the bundle and every file from the store. Each file must match the manifest hash.
3. Validate each target path:
   * relative, without `.`, `..` or `.git`;
   * **no symlink anywhere on the existing path**, re-checked after directories are created.
4. **Pre-flight decision for every file (M3).** Nothing is written if any file is in conflict.

   | On disk | Action |
   |---|---|
   | absent | `written` |
   | identical to the approved bytes | `unchanged` (idempotent) |
   | equal to the hash EAEH last wrote there (`materialized_files`) | `replaced` (newer approved version) |
   | anything else | **conflict**: *"modified locally since EAEH wrote it"* or *"exists and was not written by EAEH"* |
   | anything else, with `sync --replace-modified` (human, audited) | `replaced_modified` |

5. Write each file atomically:
   * temporary `.eaeh-tmp-*` file in the target directory;
   * `fsync`, then `rename`;
   * **re-read and re-hash**.

   The `BeforeFileModification` and `AfterFileModification` hooks run for each file. A blocking hook can veto a write; hooks cannot relax the checks.
6. Files of the previous version in the same folder that the new version no longer contains are removed if EAEH wrote them and they are unchanged. Edited ones are kept and reported as `kept_modified`.
7. Update `.eaeh/artifacts.json` atomically.
8. Record the attempt:
   * an `artifact_materializations` row with status `succeeded`, `conflict` or `failed`;
   * the audit entry `artifact.materialize`, with outcome succeeded, denied or failed, and the files and their actions;
   * the event `artifact.materialized`.

**Verification (`Materializer::status`)** re-hashes every recorded file when it is called. It reports one state:

| State | Meaning |
|---|---|
| `verified` | every file is present and equal |
| `modified` | a file differs from the approved version |
| `missing` | a file is missing |
| `outdated` | the folder holds another version |
| `conflict` / `failed` | the last attempt for this version |
| `not_materialized` | nothing has been written yet |
| `awaiting_approval` | the version is not approved, so it will not be written |
| `no_workspace` | no workspace is registered |

**`verified` is never reported unless the files were re-hashed in that call.**

**Provenance index** `.eaeh/artifacts.json` has one entry per folder:
* artifact id, name, version, hash and creation time;
* workflow instance and gate;
* approval: id, approver, role, time and approved hash;
* sources: the earlier approved gates (artifact id, version and hash; the LLD lists the HLD and the requirements) and the BRD citation when the input names one;
* `materialized_at`;
* `files {path → hash}`.

The documents themselves stay byte-identical to the approved artifacts (M4).

## 6. Workspace registration

The migration `0006_artifact_materialization.sql` adds new tables only:
* `project_workspaces`;
* `materialized_files`;
* `artifact_materializations`.

The registered root is canonicalized. Each registration is audited as `project.workspace_register`.

| Registered by | When |
|---|---|
| `eaeh project init` | bootstrap (idempotent; re-run to register an existing project) |
| `eaeh workflow run …` | every start, from the repository it runs in (must equal the manifest's root) |
| `eaeh artifact sync` | reconciliation, from inside the repository |

## 7. CLI, MCP, API and portal

| Surface | What it shows or does |
|---|---|
| `eaeh artifact status [RUN-ID]` | For each gate:<br>• artifact name, version, id and hash;<br>• approval, with approver and time, or "not approved" / "NOT VALID: …";<br>• workspace folder and state with an explanation, last write time, last failed attempt and each file not verified.<br>Also shows the location of the authoritative store and the workspace root. |
| `eaeh artifact sync [RUN-ID] [--replace-modified]` | Registers this repository, then writes the approved version of every document. Exit code 8 if any document fails or conflicts. A blocked run then resumes. Unapproved versions are `skipped`. Requires `workflow.start` (the developer, as for starting the run). |
| `eaeh workflow status` | Adds a "Documents in the project workspace" section, `BLOCKED at '<gate>': …` with the `eaeh artifact sync` fix, and `documents` / `blocked` in JSON. |
| MCP `eaeh_workflow_status` | Returns the same report (read-only). AI clients get no sync or write capability. |
| `GET /v1/workflows/{id}/detail` | Adds `documents[]` (`GateDocument`: artifact, approval check, `target_dir`, `materialization`, `blocked`) and `workspace`. |
| `GET /v1/approvals/{id}/document` | Adds `workflow.workspace_document` and `workflow.workspace`. |
| Portal, workflow page | **Documents in the project workspace** card:<br>• document, type and version with hash;<br>• approval;<br>• workspace path;<br>• materialization state with explanation and non-verified files;<br>• last written.<br>Plus a red **Blocked** banner and a `Blocked` status badge. |
| Portal, review page | A **Project workspace** card: where the approved version will be written and its current state. Drafts are never written there. |

No mock data is used. The portal shows only states the backend computed by re-hashing the files.

## 8. Fixing an existing project (the reported case)

Run these in 📁 the project repository (`simple-springboot-api`):

```bash
eaeh artifact status          # shows: workspace NOT REGISTERED; each document NO_WORKSPACE / not written
eaeh artifact sync            # registers this repository, writes docs/architecture/{requirements,hld,lld}
eaeh artifact status          # every document VERIFIED
git add docs/architecture .eaeh/artifacts.json   # commit when you are ready
```

The first command run with the new build applies migration `0006`, which adds three tables and changes no existing data.

## 9. Tests

| Test | What it proves |
|---|---|
| `tests/tests/materialization.rs` (8 tests) | • **Persistence:** artifacts persisted with hash and provenance before approval; nothing in the workspace before approval.<br>• **Written per approval:** each approval writes exactly that bundle before the next stage, every file byte-equal to the stored hash, no duplicates or temporary files.<br>• **Index:** artifact, version, hash, approval, approver and sources (LLD ← HLD hash).<br>• **Audit and events:** 3 succeeded `artifact.materialize` entries and 3 `artifact.materialized` events.<br>• **Idempotent:** sync leaves every file `unchanged`. |
| | • **Local edits:** `modified`, never `verified`; plain sync gives a conflict (audited denied, file untouched); `--replace-modified` restores the file (audited); a deleted file is `missing` and restored. |
| | • **No workspace:** the gate blocks (`running`, `blocked`, `workflow.blocked` audited once, failed attempt recorded, HLD not started); re-drive stays blocked; register + sync writes, verifies and continues to the HLD gate. |
| | • **Unrelated file:** not overwritten (all or nothing, no index written); explicit override continues. |
| | • **Symlink escape:** refused; nothing is written outside the workspace. |
| | • **Definition validation:** rejects `../`, absolute, `.git`, `.eaeh`, `//` and `./` targets. |
| | • **Hash-bound prerequisite:** an LLD prerequisite with an outdated hash fails the run before any LLD is generated. |
| | • **Authorization:** reading documents needs `workflow.read`; sync needs `workflow.start` (the approver is refused). |
| `tests/tests/portal.rs` (extended) | A full run through the portal API shows three `verified` documents with file states and times. After a file is deleted, the workflow detail and the LLD review document both report `missing`. |
| `portal/src/test/workspace.test.tsx` (4 tests) | "verified" appears only for the backend's verified state; modified files are listed; conflicts and blocked runs are explained; sync is prompted; an unregistered workspace and the `Blocked` badge are shown. |
| Mutation check | Disabling the conflict rule makes 2 materialization tests fail. |
| Existing suites | All other test suites register a workspace in the shared fixture; the migration lists now include 6. |

**Pass/fail:** `scripts/ci.sh` must pass:
* fmt;
* clippy with `-D warnings`;
* every test;
* cargo-deny.

The portal checks are `npm --prefix portal test` and `npm --prefix portal run build`.

## 10. Limitations
* **Local filesystem only.** The process that resumes the workflow must see the registered root. A remote API server blocks the gate with a clear reason until `eaeh artifact sync` is run on the machine with the repository.
* **One registered root per project.** Re-registering a different root (for example a moved repository) is allowed and audited.
* **No Git operations.** EAEH does not commit; the developer commits `docs/architecture/` and `.eaeh/artifacts.json`.
* **Design documents only.** Delivery source bundles (the early Phase 4 part) still use the explicit `eaeh artifact export`, unchanged.
* **AI clients can still edit files outside EAEH** (documented limit, ADR-0025). Such edits appear as `modified` and are never treated as approved.
