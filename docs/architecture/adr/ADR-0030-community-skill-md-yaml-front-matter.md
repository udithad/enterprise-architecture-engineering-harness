# ADR-0030: Community `SKILL.md` with YAML front matter

| Field | Value |
|---|---|
| Status | Accepted (plan `docs/implementation/stages/COMMUNITY-SKILL-FORMAT-PLAN.md`, decisions Y1–Y2 approved 2026-11-16) |
| Date | 2026-11-16 |
| Supersedes | ADR-0028 K1 and ADR-0029 O1, in part. TOML front matter is no longer the only Markdown format. |
| Phase | Supplementary fix within the Phase 1–3 scope |

## Context
Skills from community repositories use `SKILL.md` with **YAML** front matter between `---` lines: `name`, `description`, optionally `license` and `metadata`, and often other properties. The body is the Markdown instructions. EAEH accepted only TOML (`+++`) and JSON with an EAEH-specific schema, so such skills, for example the supplied `architecture-designer`, were reported invalid and never registered.

## Decision
1. **YAML front matter is the primary Markdown format.**
   * **Required:** `name` (a lowercase identifier, used as the skill id) and `description`.
   * **Validated:** the YAML syntax, that it is a single mapping, and the body (non-empty, ≤ 32 KiB).
   * **Preserved verbatim:** every other property (`license`, `metadata`, `allowed-tools`, …) in `frontmatter`, which is covered by the approval digest and shown in the portal.
   * **Nothing is rejected for being unknown.**
   * **Version:** `metadata.version`, else `version`, else `1.0.0`. Partial versions are normalised (`2` → `2.0.0`, `v1.2` → `1.2.0`).
   * **Template-like text:** `{{` in the body is neutralised (`{ {`) instead of rejected. Skill text is never rendered as a template.
2. **Compatibility.**
   * TOML (`+++`) and JSON skills are unchanged, and their digests do not change.
   * The description limit rises from 2000 to 4096 bytes, to fit community descriptions.
3. **Layers (Y2).** In order:
   1. explicit `metadata.eaeh-task-kinds` or `task_kinds`;
   2. inferred from `metadata.scope`, with a fixed table (`SCOPE_LAYERS`):
      * design, architecture → HLD and LLD;
      * requirements, analysis → requirements analysis and review;
      * planning → implementation planning;
      * implementation, code, development → implementation;
   3. otherwise none.

   The source is recorded in `layers_from` and shown before approval. A skill with no layer can be approved but is applied nowhere.
4. **Dependency (Y1).** `yaml-rust2` 0.13 with `default-features = false`:
   * pure Rust, MIT OR Apache-2.0, maintained, YAML 1.2;
   * it brings in `arraydeque` and `hashlink`;
   * it doesn't link the C-translated `unsafe-libyaml`;
   * it passes `cargo-deny`.
5. **Unchanged:**
   * discovery, nested resources, tree-hash versioning (ADR-0029);
   * the approval flow and digest binding;
   * the rule that only approved versions bind to agents.

## Consequences
* Community skills can be copied into `config/skills/<name>/` unchanged.
* Their reviewers see the full front matter, the layers and their source, and every file.
* Whether a scope-inferred layer is appropriate is a reviewer's decision at approval time.

## Alternatives considered
* **`serde_norway` or `serde_yaml_ng`.** Rejected: both rely on `unsafe-libyaml`.
* **Explicit layers only.** Rejected by Y2: community skills would apply nowhere without editing them.
* **Rejecting unknown properties.** Rejected: it contradicts the community format.
