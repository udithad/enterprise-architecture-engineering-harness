# Community `SKILL.md` format (YAML front matter) – plan

| Field | Value |
|---|---|
| Status | IMPLEMENTED 2026-11-16 – Y1 yaml-rust2, Y2 explicit else inferred from scope; see ADR-0030 |
| Date | 2026-11-16 |
| Scope | Supplementary fix within Phases 1–3. No roadmap changes. The request reverses ADR-0029 O1 ("TOML only"), so a new ADR-0030 will supersede that part. |

## 1. Findings (from source)
* **Parser.** `SkillDefinition::from_markdown` accepts **only** `+++` TOML front matter.
  * It rejects unknown keys.
  * It requires EAEH fields: `id`, `version`, `description`, plus `task_kinds` or `capabilities`.
  * The `instructions` field is the body.
* **Discovery.** `repository_skills` treats a `.md` file as a skill only if it starts with `+++`. `skill_directories` needs a `SKILL.md`; nested files are already captured (ADR-0029).
* **Result for the supplied `config/skills/architecture-designer/`.** The skill uses `---` YAML front matter with `name`, `description`, `license` and a nested `metadata` (author, company, `version: "1.1.1"`, domain, triggers, role, scope, …). Discovery reports it as **invalid** ("must start with a `+++` line"), so it is never registered.
* **Not affected.** The registry, versioning, approvals, portal viewer and agent binding work on the parsed `SkillDefinition` and need no change beyond the parser.

## 2. Design
* **Formats accepted:**
  * **YAML front matter** between `---` lines (community `SKILL.md`), the primary format;
  * TOML between `+++` lines and JSON skills, unchanged for compatibility.
* **Required:** `name` (kebab-case, used as the skill id) and `description`. Validation also checks the YAML syntax and that the front matter is a mapping.
* **Optional and preserved:** `license`, `metadata` (any nested values) and **any other properties**. These are stored verbatim as `frontmatter` in the definition, so they are covered by the approval digest and shown in the portal. Nothing is rejected for being unknown.
* **Version:**
  * `metadata.version`, else top-level `version`, else `1.0.0`;
  * a non-semver value such as `"1.1"` is normalised to `1.1.0`;
  * changed content keeps the automatic patch versioning of ADR-0029.
* **Body:** the Markdown after the closing `---` is the instruction text. Nested resources (`references/`, `examples/`, `scripts/` …) are captured as today. Text references are sent to agents within the 64 KiB budget; scripts and binaries are never sent.
* **Activation:** unchanged. Only approved versions bind to agents.

## 3. Decisions for approval

| # | Decision | Recommendation | Alternative |
|---|---|---|---|
| Y1 | YAML parser dependency | **`yaml-rust2` 0.13**, a pure-Rust YAML 1.2 parser (MIT OR Apache-2.0, maintained), with `default-features = false`. It doesn't link `unsafe-libyaml`. Its tree is converted to JSON in our code. | `serde_norway` 0.9 (serde-based fork of `serde_yaml`), which depends on `unsafe-libyaml-norway`, a C-translated parser. |
| Y2 | Which EAEH layers a community skill applies to (community skills don't name EAEH stages) | In order:<br>1. **Explicit**, if present: `metadata.eaeh-task-kinds` (or top-level `task_kinds`);<br>2. **Inferred** from `metadata.scope` with a fixed, documented table: `design`, `architecture` → `hld-design`, `lld-design`; `requirements`, `analysis` → `requirements-analysis`, `requirements-review`; `planning` → `implementation-planning`; `implementation`, `code`, `development` → `implementation`;<br>3. otherwise **no layer**: the skill can be approved but is not applied, and the portal says so.<br>The reviewer sees the layers and their source before approving. | Explicit key only: community skills apply to no layer until someone edits the file (which creates a new version). |

## 4. Tests (planned)
* **Parser:**
  * the supplied `architecture-designer/SKILL.md`;
  * representative community variants:
    * minimal (`name` and `description` only);
    * extra top-level properties (`allowed-tools`, `model`, lists, nested maps);
    * `version` at top level or in `metadata`, including non-semver;
    * CRLF line endings;
    * BOM.
  * Errors:
    * missing `name` or `description`;
    * invalid YAML;
    * a non-mapping front matter;
    * an unclosed block;
    * an empty body;
    * a name that is not a slug.
  * TOML and JSON skills keep their digests.
* **Discovery:**
  * `architecture-designer` is registered pending at 1.1.1, with all 6 files;
  * its layers are inferred from `scope: design`;
  * a reference edit creates a new version.
* **Activation:**
  * pending → not in the HLD request;
  * approved → the HLD agent receives the body and the reference files, sourced from the YAML skill.
* **Portal API:** the review shows the front matter (license, metadata) and the files. Vitest covers the front matter display.
* **`scripts/ci.sh`** must pass, including `cargo-deny` with the new dependency.
