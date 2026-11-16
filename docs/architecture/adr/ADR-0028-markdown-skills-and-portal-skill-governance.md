# ADR-0028: Markdown skills, layer skills, and skill governance in the portal

| Field | Value |
|---|---|
| Status | Accepted (plan `docs/implementation/stages/MARKDOWN-SKILLS-PLAN.md`, decisions K1–K4 approved 2026-11-15) |
| Date | 2026-11-16 |
| Amends | ADR-0026 §4 (portal decisions): skills become decidable in the portal. Standards stay CLI-only. |
| Phase | Supplementary enhancement within the Phase 1–3 scope |

## Context
Skills could only be written as JSON, with the instructions held in one long string field. Additional skills for a layer (requirements, HLD, LLD, plan, implementation) were applied only if someone registered them by hand. The portal could not register or decide skills.

## Decision
1. **Markdown skill files (K1).**
   * `config/skills/*.md` with **TOML front matter** between `+++` lines, holding the same metadata as JSON skills.
   * The **Markdown body is the instruction text**, verbatim.
   * Defaults: `capabilities` = `task_kinds`, `max_risk` = `read_only`.
   * Unknown front matter keys are rejected.
   * The parsed definition is validated and digested exactly like JSON, so the approval binds to the definition.
   * Markdown files without front matter (a README, for example) are ignored.
   * The same id and version defined twice is an error.
2. **Layer skills (K2).**
   * A skill applies to every workflow stage whose task kind is in its `task_kinds`, through the existing dynamic selection.
   * `project init` and `skill sync` register these skill files too, as pending.
   * Readiness: a pending required (agent) skill blocks; a pending layer skill warns.
   * The registry stays installation-wide.
3. **Portal registration (K3).**
   * `eaeh portal --steward <profile>` (an explicit `skill.manage` identity) enables **Register skill files**. It registers unregistered skill files through the same service as `eaeh skill sync` (`POST /v1/skills/sync`).
   * No upload or editing in the browser.
4. **Portal decisions on skills (K4).**
   * The `--approver` identity may decide skill approval requests.
   * The review page shows the full definition, the layers it applies to, and the digest.
   * Decisions are bound to the digest and use the unchanged approval service.
   * The registrant can never approve.
   * Mandatory standards remain `not_available_in_portal`.

## Consequences
* Authors write skills as readable Markdown. Approved skills reach each matching layer automatically and are recorded in artifact provenance (`provenance.skills`).
* The portal now carries a third, optional stored identity, the steward. The local-guard and secret model of ADR-0026 applies to it unchanged.
* Changing a skill file without changing its version cannot be registered, because versions are immutable. The portal shows such a file as *File changed – bump version*.

## Alternatives considered
* **YAML front matter.** Rejected: it needs a new YAML dependency, and `serde_yaml` is deprecated.
* **Project-local skills.** Deferred: they need a project scope in the skill registry.
* **Upload or paste skills in the browser.** Rejected: it creates skills without a repository source, and puts arbitrary text one approval away from becoming instructions.
* **Read-only skills in the portal.** Rejected: the user asked for registration and approval through the portal.
