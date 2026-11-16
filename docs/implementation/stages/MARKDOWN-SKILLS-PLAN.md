# Markdown skills, per-layer skill use, and skill governance in the portal – plan

| Field | Value |
|---|---|
| Status | IMPLEMENTED 2026-11-16 – K1–K4 as recommended; see docs/implementation/EAEH-MARKDOWN-SKILLS.md and ADR-0028 |
| Date | 2026-11-15 |
| Scope | Supplementary enhancement within Phases 1–3 (Phase 2 skills, Phase 3 design, ADR-0026 portal). No roadmap changes. |

## 1. Current state (from source)
* **Format.**
  * Skills are JSON files in `definitions.skills_dir` (`config/skills/*.json`).
  * `SkillDefinition`: id, version, description, capabilities, task_kinds, `instructions` (plain text), required_knowledge, required_tools, max_risk, input/output schema, dependencies, evaluation_criteria.
  * `repository_skills()` reads only `*.json`.
* **Registration.**
  * `eaeh project init` and `eaeh skill sync` register the skills the agents of the project's workflows reference (`required_skills`).
  * `eaeh skill register --file x.json` registers one skill.
  * Every skill starts `pending_approval` and needs a human approver.
  * The registry is installation-wide (`skills` table, keyed by id and version).
* **Use per layer.** `SkillRegistry::resolve` binds two kinds of skill to an agent run:
  * the agent's **explicit** skills (`hld-design@^1`);
  * every **approved** skill whose `task_kinds` contains the stage's task kind (dynamic selection).

  Each bound skill's instructions enter the context as `APPROVED SKILL [id@version]`, and the skill references are recorded in the artifact's provenance. The stage task kinds are:

  | Workflow | Stages (task kind) |
  |---|---|
  | `requirements.analysis`, `engineering.design` | `analyse` (`requirements-analysis`), `refine` (`requirements-review`) |
  | `engineering.design` | `hld` (`hld-design`), `lld` (`lld-design`) |
  | `engineering.delivery` | `plan` (`implementation-planning`), `implement` (`implementation`) |

  **Gap:** an *additional* skill for a layer is applied only if someone registers it manually. `skill sync` ignores skill files that no agent references.
* **Portal.** ADR-0026 §4 allows decisions on **workflow gates only**. Skills and standards are decided with `eaeh approval review`, and the portal cannot register anything.

## 2. Decisions for approval

| # | Decision | Recommendation | Alternative |
|---|---|---|---|
| K1 | Markdown skill format | `*.md` with **TOML front matter** between `+++` lines. It holds the same metadata as JSON skills: id, version, description, task_kinds (the layers), capabilities, required_knowledge, required_tools, max_risk, optional output_schema. The **Markdown body is the instructions**, verbatim. No new dependency: `toml` is already used for `project.toml`. JSON skills keep working unchanged. | YAML front matter (`---`), the style of other skill ecosystems. It needs a new YAML dependency (`serde_yaml` is deprecated), which must be evaluated. |
| K2 | Where skill files live, and per-layer use | Skill files live in the installation's `config/skills/`, applying to all projects as the registry already does. `skill sync` and `project init` register **every** skill file whose `task_kinds` match a stage of the project's workflows, plus the agents' explicit skills. Once approved, a skill is applied automatically at every matching layer and recorded in provenance. Readiness: a pending **required** skill still blocks; a pending **additional** skill only warns. The portal and `eaeh skill pending` show the skills of each layer. | Also project-local skills in the repository (`.eaeh/skills/*.md`). This needs a project scope on skills (schema and resolver change), because the registry is installation-wide today and a project skill would otherwise apply to every project. |
| K3 | Registering skills through the portal | A **"Register skill files"** action registers new or changed skill files from `config/skills/` as **pending approval**, through the same `SkillRegistry::register`, validation and audit as the CLI. It needs an explicit `eaeh portal --steward <profile>` identity (`skill.manage`), mirroring `--approver`. The browser cannot edit or upload skill text. | Also paste or upload a Markdown skill in the portal. That creates a skill with no file in the repository, and puts arbitrary text one approval away from becoming instructions. |
| K4 | Approving skills through the portal (amends ADR-0026 §4) | Allow portal decisions on **skill approvals**, as the `--approver` identity only. The review page shows the **full skill**: metadata, the layers it applies to, required tools and knowledge, the rendered Markdown instructions, and the digest. Decisions are Approve, Request changes or Reject, with comments and confirmation. They are bound to the skill's digest (stale-digest refusal) and use the same approval service, so separation of duties (the registrant cannot approve), audit and status changes are unchanged. Standards stay CLI-only. Recorded in a new ADR-0028. | Keep skills CLI-only; show them read-only in the portal. |

## 3. Implementation (after approval)
* **`eaeh-skills`:** `SkillDefinition::from_markdown(text)`, which splits the front matter from the body, applies defaults (`capabilities` defaults to `task_kinds`, `max_risk` to `read_only`) and validates. Unit tests cover the parser.
* **Runtime (`repository_skills`):** reads `*.json` and `*.md`, and rejects a duplicate id and version. `required_skills` adds the task-kind-matched skill files. Readiness distinguishes required from additional skills.
* **CLI:**
  * `eaeh skill register --file x.md` (JSON or Markdown);
  * `eaeh skill pending` lists the skills per layer;
  * `eaeh skill sync` registers the additional skills.
* **API, as read models in `views.rs`:**
  * `GET /v1/skills/catalog`: files, registered versions, status, layers, the approval request;
  * `GET /v1/approvals/{id}/document` for skill approvals: full content and digest;
  * `POST /v1/skills/sync`: registration, which calls the shared runtime function.
* **Portal server:**
  * `--steward` profile;
  * allowlist: `POST /v1/skills/sync` with the steward token, and decisions on skill approvals with the approver token. Standards remain refused.
* **Portal UI:**
  * a **Skills** page: the skills per layer, their status, "Register skill files" and Review links;
  * the skill review page, using the existing decision panel with a digest pre-check.
* **Example:** convert `config/skills/hld-design.json` into a Markdown example? **No.** The existing JSON skills stay unchanged so that approved digests stay valid. A new **example Markdown skill** for the HLD layer (`config/skills/architecture-diagrams.md`) shows the format; it is registered as pending, and a human approves it.
* **Docs:**
  * ADR-0028;
  * `docs/implementation/EAEH-MARKDOWN-SKILLS.md`;
  * a skills section in `EAEH-USER-GUIDE.md` covering the format, layers, CLI and portal steps.

## 4. Tests (planned)
* **Parser:** valid file; missing or invalid front matter; body becomes the instructions; defaults; invalid id, version or risk.
* **Loader:** JSON and Markdown together; duplicates rejected.
* **Sync:** registers additional layer skills as pending, idempotently; a changed file is a new version request.
* **Resolution:** an approved Markdown skill is bound to the HLD stage of a design run (its instructions appear in the hand-off and model context, and its reference in the artifact provenance). A pending or rejected one is not bound.
* **Portal:**
  * register with `--steward` creates pending skills (audited);
  * without `--steward` it is refused;
  * a skill decision as the approver changes the status;
  * a stale digest is refused;
  * the steward cannot approve their own registration;
  * standards remain refused.
* **Vitest:** Skills page; skill review rendering; decision panel for skills.
* **Existing suites stay green:** `scripts/ci.sh`.
