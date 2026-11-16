# ADR-0029: Skill directories, automatic discovery and content-bound versions

| Field | Value |
|---|---|
| Status | Accepted (plan `docs/implementation/stages/SKILL-ONBOARDING-PLAN.md`, decisions O1–O4 approved 2026-11-16) |
| Date | 2026-11-16 |
| Extends | ADR-0028 (Markdown skills, portal skill governance) |
| Phase | Supplementary enhancement within the Phase 1–3 scope |

## Context
Skills could only be single files, registered explicitly. Reviewers could not see a skill's supporting material. A file changed after approval could not be registered without a manual version bump.

## Decision
1. **Skill directory (O1).** A skill can be a folder: `config/skills/<name>/SKILL.md` plus nested folders and files.
   * The manifest is TOML front matter in `SKILL.md` (ADR-0028), whose body is the main instructions.
   * Every file is captured into the definition's `package`: path, SHA-256, size, kind, and the text content (binaries as hash only).
   * The definition **digest covers every file**, so an approval covers the whole folder.
   * Limits: 2 MiB per folder, 512 KiB per file, 500 files. Symlinks and special files are refused.
2. **Content-bound versions (O2).** A `tree_hash` identifies the folder content.
   * If the content changes while the manifest version is already registered, discovery registers the next free patch version, recording the manifest version and a revision number.
   * The approved version stays in force until a person approves the new one, because only approved versions bind to agents.
   * History stays in the approval service.
3. **Automatic discovery (O3).**
   * **When it runs:** `eaeh portal` and `eaeh serve` scan at startup and every 30 s (`EAEH_SKILL_DISCOVERY_INTERVAL_SECS`). The CLI commands `project init`, `skill sync`, `skill pending` and `project readiness` also scan, and the portal has **Scan now**.
   * **Registrant:** the trusted platform component `system:skill-discovery`. `SkillRegistry::register_discovered` uses the same validation, approval request, events and audit as `register`.
   * **Approval scope:** the triggering project, else the project where skills were last governed.
   * A rejection of a skill must carry comments, a rule enforced by the approval service.
4. **What agents receive (O4).**
   * The `SKILL.md` body, plus the text reference files as skill-trust context within a 64 KiB budget per skill.
   * Code and binaries are listed by path only, and are never executed or sent.

## Consequences
* Copying a folder into `config/skills/` is enough to propose a skill. Nothing reaches an agent before a human approves exactly that content.
* The `skills` table is unchanged. Folder content is stored in the definition document, and single-file skills keep their digests.
* A registrant that is not a person exists for skills. Separation of duties is preserved: the approver must be a human with the approver role, and is never the registrant.

## Alternatives considered
* **YAML front matter.** Rejected: it needs a new dependency.
* **Requiring a manual version bump.** Rejected: an unnoticed edit would silently diverge from the approved content, or be blocked.
* **Discovery by the steward only.** Rejected: the request asks for automatic discovery.
* **Sending only `SKILL.md` to agents.** Rejected: reference documents are part of the skill.
