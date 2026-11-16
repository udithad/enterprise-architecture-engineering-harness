# Markdown skills, layer skills and skill governance in the portal (supplementary)

> **Supplementary document.** This is an enhancement within Phases 1–3. The eight-phase roadmap, phase definitions, next steps and acceptance criteria are unchanged.
>
> | | |
> |---|---|
> | Plan (decisions K1–K4) | `docs/implementation/stages/MARKDOWN-SKILLS-PLAN.md` |
> | Decision | [ADR-0028](../architecture/adr/ADR-0028-markdown-skills-and-portal-skill-governance.md), which amends ADR-0026 §4 |
> | User steps | [EAEH User Guide §10](../user-guides/EAEH-USER-GUIDE.md#10-skills-write-register-approve) |

## What changed

| Area | Change |
|---|---|
| Format (`eaeh-skills`) | `SkillDefinition::from_markdown` and `from_file_text`:<br>• TOML front matter between `+++` lines, with the Markdown body as the instructions;<br>• defaults `capabilities` = `task_kinds` and `max_risk` = `read_only`;<br>• unknown keys rejected, and `instructions` is not allowed in the front matter;<br>• validated and digested like JSON skills. |
| Loading (`runtime::project`) | `repository_skills` reads `*.json` and `*.md`. Markdown without front matter (a README) is skipped. The same id and version defined twice is an error that names both files. |
| Layers | `workflow_layers` lists each workflow stage with its task kind. `layer_skills` finds the skill files that apply to a project's stages. `project_skill_references` = the agents' skills + the layer skills, and is used by `project init` and `skill sync`. Readiness: a pending layer skill is a **warning**; a pending required skill still blocks. |
| Execution | Unchanged: `SkillRegistry::resolve` binds approved skills by task kind and adds `APPROVED SKILL [id@version]` instructions. The skill references are recorded in `provenance.skills`. |
| CLI | `skill register --file x.md`; `skill pending` (adds `layer_skills` with their layers); `skill sync` (registers layer skills). |
| MCP | `eaeh_project_context` adds `layer_skills`. |
| API | `GET /v1/skills/catalog` (`skill.read`): files and registrations, layers, status, `file_changed`.<br>`POST /v1/skills/sync {project_id}` (`skill.manage`): registers the unregistered skill files, pending approval.<br>`GET /v1/approvals/{id}/document` adds `skill` (full definition and digest) for skill approvals. |
| Portal server | `eaeh portal --steward <profile>`. `POST /v1/skills/sync` uses the steward token, and returns `steward_not_configured` without it. Skill approvals are decidable by the approver; mandatory standards are still refused. The session reports `steward` and `can_register_skills`. |
| Portal UI | **Skills** page: files, layers, format, status, Register skill files, Review links, and the task kind → stage map. The approval review page shows the full skill (`SkillReview`). The decision panel accepts skills and is bound to the digest. |
| Example | `config/skills/architecture-diagrams.md` (HLD and LLD layers). It is registered as pending by `project init`/`skill sync`; a human approves it. |

## Tests

| Test | Proves |
|---|---|
| `eaeh-skills` `markdown_tests` (2) | • the parser, the defaults, and a line-ending-independent digest;<br>• 10 malformed cases are rejected. |
| `tests/tests/skills.rs` (2) | **Layer application:**<br>• a **pending** Markdown skill is not in the HLD run;<br>• after a human approval it is in the LLD run, with its Markdown text and in `provenance.skills`;<br>• it is never in the requirements runs.<br>**Loader:** JSON and Markdown together; README ignored; layer skills found; duplicates refused. |
| `tests/tests/portal.rs` `portal_registers_and_decides_skills_but_not_standards` | • the catalog shows the layers;<br>• registration needs `--steward`, and then registers idempotently with audit;<br>• the review shows the full skill and its digest;<br>• a stale digest is refused;<br>• the steward cannot approve;<br>• the approver approves, and the catalog shows approved;<br>• a mandatory standard stays `not_available_in_portal`. |
| `crates/eaeh-cli/tests/dx.rs` (updated) | `project init` registers the layer skill as well. `skill pending` lists it with its layers. |
| `portal/src/test/skills.test.tsx` (3) | • Skills page rows and registration as the steward;<br>• the hint shown without a steward;<br>• the skill review, and the status mapping. |

## Limitations
* **Skills are installation-wide.** There are no project-local skill files, because that needs a project scope in the registry (decision K2 deferred it).
* **The browser cannot edit or upload skills.**
* **A changed file needs a new `version`.** Registered versions are immutable.

## Skill directories and onboarding (added 2026-11-16)

These are decisions O1–O4 of `docs/implementation/stages/SKILL-ONBOARDING-PLAN.md`, recorded in [ADR-0029](../architecture/adr/ADR-0029-skill-directories-and-automatic-discovery.md).

| Area | Change |
|---|---|
| `eaeh-skills::package` | `read_dir_files`: a nested walk with symlinks and special files refused; limits of 2 MiB per folder, 512 KiB per file and 500 files; hidden entries skipped.<br>File kinds: markdown, json, yaml, toml, text, code, binary.<br>`tree_hash` is order-independent. `validate` re-checks each content hash and the tree hash. |
| `SkillDefinition` | Optional `package` (dir, source_version, revision, tree_hash, files). Single-file skills don't serialize it, so their digests are unchanged.<br>`from_directory` loads `SKILL.md` plus all files. |
| `SkillRegistry` | `register_discovered` follows the same path as `register` (validation, approval request, events, audit `skill.register` with `discovered` and the file list), with the registrant `system:skill-discovery`. `all_records` is for trusted components. `instruction_items` adds the reference text files (64 KiB budget) and lists the other files. |
| Approval service | Rejecting a skill requires comments (`invalid_input` otherwise). Other subjects are unchanged. |
| `runtime::project` | `skill_directories`. `discover_skills` handles new, changed (next free patch version, with revision), unchanged and invalid folders, idempotently. `spawn_skill_discovery` and `skill_discovery_interval` (30 s, or `EAEH_SKILL_DISCOVERY_INTERVAL_SECS`). Layer skills include folders (`id@^MAJOR`). |
| Triggers | `eaeh portal` and `eaeh serve` (startup and periodic); CLI `project init`, `skill sync` (also prints the folders), `skill pending`, `project readiness`; portal **Scan now**. |
| API | `POST /v1/skills/discover` (`skill.read`).<br>`GET /v1/skills/{id}/{version}/files` (`skill.read`): every file with content, kind, hash and `change` against the version in force; `removed_files`; approval `history`.<br>The catalog lists every registered version with `in_force`, `package` and `format: directory`, plus folders that are not registered yet, under `directories`. |
| Portal server | Allowlists `POST /v1/skills/discover` (viewer token) and `GET …/files`. |
| Portal UI | **Skills** page:<br>• **Skill directories** card with Scan now, the discovery results and invalid folders;<br>• in-force badge, file count and viewer link per version.<br>**Skill viewer** (`/skills/:id/:version`):<br>• metadata (layers, version in force, manifest and revision, tree hash, digest);<br>• `SkillFiles`: folder tree and previews (Markdown rendered, JSON pretty-printed, YAML/TOML/code/text as numbered lines, binaries by hash), added/modified markers, removed files;<br>• approval history;<br>• **Review and decide** link.<br>The review page embeds the file browser. Skills get Approve or Reject, with no request changes. |
| Example | `config/skills/api-error-handling/`: an LLD layer skill with `references/` (nested `http/`), `examples/` (JSON and YAML) and `scripts/` (never run). It is discovered as pending, and a human approves it. |

### Tests

| Test | Proves |
|---|---|
| `eaeh-skills` `package::tests` (2) | • nested capture with kinds, binaries kept as hash only, hidden files skipped;<br>• an order-independent tree hash that a nested edit changes;<br>• symlinks and oversized files refused. |
| `tests/tests/skills.rs` – `skill_directories_are_discovered_versioned_on_change_and_only_approved_versions_apply` | • **discovery:** an empty folder registers nothing; a copied folder becomes `new` 1.0.0, pending, registered by `system:skill-discovery` with all 6 files and a single audit record; a re-scan reports `unchanged`;<br>• **pending:** while pending, no agent uses it;<br>• **after approval:** the LLD agent receives `SKILL.md` and the nested references, the script is only listed (its content is never sent), and the HLD layer doesn't receive it;<br>• **change:** a nested edit becomes `changed` 1.0.1 (revision 2, manifest 1.0.0), and 1.0.0 stays in force; reject with comments, then 1.0.0 is still in force; another edit becomes 1.0.2 (revision 3); approved, it is in force; the stored package verifies;<br>• **invalid:** an invalid folder is reported, not registered. |
| `tests/tests/portal.rs` – `portal_skill_onboarding_discovers_shows_files_and_decides_versions` | • the catalog lists a folder that isn't registered yet; Scan now registers it;<br>• the files endpoint returns the nested tree with kinds and hashes;<br>• approve in the portal;<br>• editing and adding files gives 1.0.1 with *modified* and *added* markers;<br>• a reject without a comment is refused; with a comment it succeeds;<br>• the history shows both versions; 1.0.0 stays in force. |
| `crates/eaeh-cli/tests/project_discovery.rs` – `skill_directories_copied_in_are_discovered_reviewed_and_versioned_through_the_portal` | **Real binary, running portal:**<br>• a folder copied in is discovered without a restart;<br>• its nested content is shown;<br>• approved through the portal;<br>• a nested edit gives 1.0.1, pending, compared with 1.0.0;<br>• rejected with comments;<br>• `eaeh skill pending` shows 1.0.0 approved. |
| `portal/src/test/skill-onboarding.test.tsx` (4) | • the tree builder;<br>• previews for Markdown, JSON, YAML, code and binary files, with change and removed markers;<br>• Scan now and invalid folders;<br>• the viewer, with the version in force, the review link and the history. |

### Remaining gaps
* **No browser E2E (Playwright) for the new screens.** UI behaviour is covered by vitest, and the flows by the real-binary API test.
* **No Playwright run in this change set.**
* **Skills are installation-wide.** There are no per-project skill folders.
* **One governance project per discovery run.** Discovery uses the triggering project, else the project where skills were last governed. Approvers need access to that project.
* **No YAML front matter**, by decision O1.

## Community `SKILL.md` with YAML front matter (added 2026-11-16)

This is [ADR-0030](../architecture/adr/ADR-0030-community-skill-md-yaml-front-matter.md), with plan `docs/implementation/stages/COMMUNITY-SKILL-FORMAT-PLAN.md`.

| Area | Change |
|---|---|
| Parser (`eaeh-skills`) | `from_markdown` dispatches on the first line: `---` → `from_yaml_markdown`, `+++` → TOML (unchanged). JSON is unchanged.<br>The YAML path:<br>• requires `name` (a slug, used as the id) and `description`;<br>• validates the YAML syntax and that it is a single mapping;<br>• keeps the whole front matter as `frontmatter`;<br>• version from `metadata.version`, else `version`, else 1.0.0 (lenient);<br>• layers explicit (`metadata.eaeh-task-kinds` or `task_kinds`), else from `metadata.scope` (`SCOPE_LAYERS`), else none, recorded in `layers_from`;<br>• max risk `read_only`;<br>• `{{` in the body neutralised.<br>The description limit is 4096 bytes. |
| Dependency | `yaml-rust2` 0.13 (`default-features = false`; MIT OR Apache-2.0; pure Rust) |
| Loading | Single `.md` skill files with `---` front matter are recognised. Skill directories needed no change: nested resources are captured as before (ADR-0029). |
| API and portal | `skill_view` adds `frontmatter` and `layers_from`. The review page and the skill viewer show a **SKILL.md front matter** card with every property, including nested metadata, and the source of the layers. |
| Unchanged | Discovery, versioning, approval (reject needs comments), and activation of approved versions only |

**Tests:**

| Test | Proves |
|---|---|
| `eaeh-skills` `community_skill_tests` (4) | • the supplied `architecture-designer`: version 1.1.1, layers inferred from `scope: design`, licence and metadata preserved, the digest covers the front matter;<br>• representative community variants: minimal; extra properties (`allowed-tools`, `model`, booleans, nested maps, lists); integer, top-level and `v`-prefixed versions; explicit layers as a comma string or a list; CRLF and BOM; `{{` neutralised;<br>• 9 rejections, each with its reason;<br>• TOML and JSON skills unchanged (no new fields, so the same digests). |
| `tests/tests/skills.rs` – `the_community_architecture_designer_skill_is_discovered_approved_and_applied` | • discovered as 1.1.1 with its 5 reference files;<br>• pending → not in the HLD request;<br>• approved → the LLD agent receives the body and the references, and provenance records the skill;<br>• a reference edit → 1.1.2 pending, while 1.1.1 stays active. |
| `tests/tests/portal.rs` – `portal_reviews_a_community_skill_with_its_front_matter_and_files` | • the review shows the front matter, the layer source, the instructions and all 6 files;<br>• a reject with a blank comment is refused;<br>• approve → in force. |
| `crates/eaeh-cli/tests/dx.rs` | The pending counts are now derived from the skill folders present, instead of fixed numbers. |
| `portal/src/test/skill-onboarding.test.tsx` | The front matter card and the layer source. |
