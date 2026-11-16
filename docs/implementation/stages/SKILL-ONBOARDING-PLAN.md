# Skill onboarding and governance (skill directories) – plan

| Field | Value |
|---|---|
| Status | IMPLEMENTED 2026-11-16 – O1–O4 as recommended; see ADR-0029 and docs/implementation/EAEH-MARKDOWN-SKILLS.md (§ Skill directories) |
| Date | 2026-11-16 |
| Scope | Supplementary enhancement within Phases 1–3 (Phase 2 skills; portal per ADR-0026 and ADR-0028). No roadmap changes. |

## 1. Current state (from source)
* **Skill files.** A skill is a single file in `config/skills/`, either `*.json` or `*.md` with TOML front matter (ADR-0028). `repository_skills()` reads only top-level files.
* **Registration.** `SkillRegistry::register` (needs `skill.manage`) validates the skill, stores the definition as JSON in the `skills` table (`id`, `version`, `document`, `digest`, `status`, `approval_id`), and requests a human approval bound to the **digest of the definition**. Versions are immutable, and the registrant cannot approve. Only approved (or deprecated) versions are bound to agents.
* **Discovery.** Explicit only, through `project init`, `skill sync`, or **Register skill files** in the portal. A file changed without a version bump cannot be registered; the portal shows *File changed – bump version*.
* **Portal.** Skills page with the catalog and Review links; skill approvals are decided with the full definition shown (ADR-0028).
* **Gaps against the request.**
  * No skill **directories**: no nested reference documents, and no file tree or contents viewer.
  * No **automatic** discovery.
  * The approval binds to the definition, not to **every file**.
  * No automatic new version when files change.

## 2. Decisions for approval

| # | Decision | Recommendation | Alternative |
|---|---|---|---|
| O1 | Skill directory and manifest | A **directory** `config/skills/<name>/` with a manifest `SKILL.md` (TOML front matter between `+++` lines, as in ADR-0028, whose body is the main instructions), plus any nested files and folders (`references/`, `examples/`, `scripts/` …). Single-file JSON and Markdown skills keep working unchanged. The manifest is validated as today. | Also accept YAML front matter (`---`, as in other skill ecosystems). This needs a new YAML dependency to evaluate, since `serde_yaml` is deprecated. |
| O2 | Hash, versioning and changes after approval | A **tree hash** covers every file (relative path + SHA-256 + size), stored with the version. The approval binds to it, and the portal shows each file's hash. If any file changes and the manifest version is already registered with a different tree hash, discovery registers the **next patch version automatically** (for example `1.0.0` → `1.0.1`, noting "from manifest 1.0.0, revision 2") as Pending Approval. The approved version stays in force until the new one is approved, and history is kept. | Require the author to bump `version` in `SKILL.md`. A changed directory is flagged and **not** registered. |
| O3 | When discovery runs, and who registers | Automatically on every relevant read:<br>• `eaeh portal` and `eaeh serve` scan at startup and every **30 s**;<br>• the CLI commands `project init`, `skill sync`, `skill pending` and `project readiness` scan too;<br>• the portal has **Scan now**.<br>Discovered versions are registered by the platform component `system:skill-discovery` (audited `skill.discover`). No human registrant is involved, so any approver may approve. A **human** still approves every version. | Discovery only when a steward clicks **Register skill files** or runs `eaeh skill sync`. |
| O4 | What agents receive from an approved directory skill | The `SKILL.md` body as the instructions, as today. Text reference files (`.md`, `.txt`, `.json`, `.yaml`, `.toml`, `.csv`) are added as **skill-trust reference context**, within a budget (64 KiB per skill). Anything beyond the budget is listed by path. **Scripts and binaries are never executed or sent**; they are listed only. Everything sent is part of the approved tree hash. | The `SKILL.md` body only; other files are for human reviewers. |

## 3. Implementation (after approval)
* **`eaeh-skills`**
  * `SkillDefinition` gains an optional `files` field: path, hash, size, kind and the stored text, with limits of 2 MiB per directory and 512 KiB per file. Binary files are stored as a hash only.
  * `tree_hash` is added. The `skills` table is unchanged: the file contents go into the stored `document` JSON. With an empty `files`, the field is not serialized, so the digests of existing skills do not change.
  * `register_discovered(…)`: the same validation, approval request, events and audit as `register`, attributed to `system:skill-discovery` (trusted component). Separation of duties is unchanged, because the approver must be human and not the registrant.
* **`runtime::project`**
  * `discover_skills(h)` walks `config/skills/`:
    * top-level files as today;
    * directories containing `SKILL.md`;
    * symlinks and paths outside the folder rejected;
    * size limits enforced.
  * It compares each tree hash with the registry and registers new and changed versions as pending.
  * It returns a report (`new`, `changed → 1.0.1`, `unchanged`, `invalid: reason`).
* **Engine and context.** Resolution is unchanged (approved only). A bound skill's reference files become additional skill-trust items (budgeted), produced only by `eaeh-skills` as the fitness test requires.
* **API**
  * `GET /v1/skills/catalog` adds the directory, tree hash, file count, `discovery` state and the source manifest version.
  * `GET /v1/skills/{id}/{version}/files` returns the complete tree with contents and hashes.
  * `POST /v1/skills/discover` is Scan now.
  * The existing approval document endpoint embeds the tree.
* **Portal**
  * The Skills page shows new and changed badges, Scan now and the discovery results.
  * The **skill viewer** has a directory tree (nested folders) and a file pane:
    * Markdown rendered;
    * JSON and YAML pretty-printed;
    * source code as monospace text with line numbers;
    * binary files as hash and size only;
    * each file's SHA-256;
    * a "changed since the approved version" diff marker per file.
  * Approve and Reject use the existing decision panel; reject requires a comment. Approval history comes from the approval service.
* **Example.** `config/skills/api-error-handling/`, an LLD-layer skill with `SKILL.md`, `references/error-catalog.md`, `examples/error-response.json` and `examples/config.yaml`. It is discovered as pending, and a human approves it.

## 4. Tests (planned)
* **Unit tests:** manifest validation; tree hash; directory walk (nesting, symlink refused, limits); automatic patch version; unchanged directory is idempotent.
* **Backend:**
  * discovery registers new and changed versions as pending, audited;
  * approval and rejection with comments go through the existing service;
  * only approved versions are bound;
  * a change after approval creates `1.0.1`, the approved `1.0.0` stays active until then, and history is kept;
  * reference files reach the context within the budget, and scripts never do.
* **Portal API:** catalog, files tree and contents, Scan now, decisions (approve, reject with comments, stale tree hash refused).
* **End to end with the real binary:**
  * copy a skill directory into a temporary installation while `eaeh portal` runs;
  * it appears pending without a restart (periodic scan);
  * approve and reject through the portal API;
  * edit a nested file, and a new version appears pending.
* **Vitest:** viewer (tree, Markdown, JSON, YAML and code previews, binary), Skills page states.
* **`scripts/ci.sh`** must pass.
