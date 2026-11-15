# ADR-0027: Approved design documents are materialized into the project workspace

| Field | Value |
|---|---|
| Status | Accepted (plan `docs/implementation/stages/ARTIFACT-MATERIALIZATION-PLAN.md`, decisions M1–M4 approved 2026-11-15) |
| Date | 2026-11-15 |
| Phase | Supplementary fix within the Phase 1–3 scope |
| Deciders | Product owner (defect report), Principal architect |

## Context
Approved Requirements, HLD and LLD documents existed only in EAEH's artifact store. Developers expected them as files in the application repository: in VS Code, in Git, and as input to later work.

Nothing wrote them there except a manual `eaeh artifact export`. The repository root was known only from the repo-local manifest, so the processes that resume a workflow after a decision could not find it. Those processes are the portal, `eaeh serve`, the MCP server and the worker.

## Decision
1. **The artifact store stays authoritative.** Every version, hash, provenance record, approval and audit entry stays there. The workspace holds a **materialized copy of the current approved version** only. Drafts are never written to it.
2. **Engine-driven, declared on the gate (M1).**
   * An approval gate may declare `materialize: "<repository-relative dir>"`.
   * After the approval is recorded, the engine writes the approved bundle **before** the `approved` transition.
   * If the write fails, the run is held at the gate: status `running` with `blocked` set, audited as `workflow.blocked`. It continues only after a successful retry, either on any drive or with `eaeh artifact sync`.
   * Progression never silently skips materialization.
3. **Layout (M2):** `docs/architecture/{requirements,hld,lld}/`, one folder per bundle. The BRD is not copied.
4. **Ownership (M3).**
   * EAEH replaces only files it wrote that are still unchanged.
   * Any other existing file is a conflict, and nothing in that bundle is written.
   * `eaeh artifact sync --replace-modified` is an explicit, audited human override.
5. **Provenance (M4):** `.eaeh/artifacts.json` in the repository. The documents stay byte-identical to the approved hashes.
6. **Workspace mapping.**
   * The registered root is stored in the database (`project_workspaces`, migration 0006, new tables only).
   * It is registered by `project init`, `workflow run` and `artifact sync` from inside the manifest's repository, and audited.
7. **Hash-bound prerequisites.** `requires_approval_of` uses the completion guard's gate check: the approval must hold for the **current** version of the reviewed artifact.
8. **Confinement.** Writes use the same rules as EAEH workspaces:
   * relative paths only, no `..`;
   * no symlinks on the path (re-checked after directories are created);
   * nothing under `.git`;
   * atomic temporary file + rename, with re-hash verification.

   The `BeforeFileModification` and `AfterFileModification` hooks run for each file. Hooks cannot relax these checks.

## Consequences
* Developers find approved documents in `docs/architecture/` immediately after each approval. The portal and CLI show the verified state, computed by re-hashing the files at read time, and explain anything missing.
* A run can now be held at an approved gate. This is visible everywhere as **Blocked**, with the reason and the fix.
* A process that cannot see the repository, such as a remote API server, holds the gate until `eaeh artifact sync` runs where the repository is.
* Delivery source bundles keep the explicit export. EAEH performs no Git operations.

## Alternatives considered
* **Separate `tool_step` states after each gate.** Rejected: a failure would end the run as `failed`, and the policy would be repeated in every definition instead of being one engine rule.
* **Status reporting only (keep manual export).** Rejected: it does not meet the expectation that approved documents are in the repository.
* **A new `blocked` instance status.** Rejected for now: SQLite cannot alter the status CHECK constraint without rebuilding five related tables. The `blocked` flag on a `running` instance gives the same visibility without that migration risk.
* **Front-matter provenance inside each document.** Rejected: the bytes would differ from the approved hash.
