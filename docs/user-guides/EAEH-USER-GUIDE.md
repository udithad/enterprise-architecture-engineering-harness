# EAEH User Guide

From `git clone` to approved Requirements, HLD and LLD in your own project. Follow the sections in order; each takes a few minutes.

| # | Section | How often |
|---|---|---|
| 1 | [What EAEH does](#1-what-eaeh-does) | Read once |
| 2 | [Get and build EAEH](#2-get-and-build-eaeh) | Once per computer |
| 3 | [Set up EAEH](#3-set-up-eaeh) | Once per computer |
| 4 | [Start the portal](#4-start-the-portal) | Whenever you want the web UI |
| 5 | [Try the demo (no AI, no subscription)](#5-try-the-demo-no-ai-no-subscription) | Optional |
| 6 | [Set up a new project](#6-set-up-a-new-project) | Once per project |
| 7 | [Run the design and approve](#7-run-the-design-and-approve) | Every design |
| 8 | [Day-to-day: CLI and portal side by side](#8-day-to-day-cli-and-portal-side-by-side) | Look up |
| 9 | [Troubleshooting](#9-troubleshooting) | When something stops |
| 10 | [Skills: write, register, approve](#10-skills-write-register-approve) | When you add or change a skill |
| 11 | [Reference](#11-reference) | Look up |

Every command block starts with a 📁 line saying **where** to run it:
* **EAEH folder** is the clone of this repository;
* **project folder** is your application repository;
* **any folder** means it doesn't matter.

Replace `<ANGLE-BRACKETS>` with your own value.

---

## 1. What EAEH does

```
BRD ─► Requirements ─► [approve] ─► HLD ─► [approve] ─► LLD ─► [approve]      activity "design"
```

| Step | Who |
|---|---|
| Writes requirements, HLD and LLD | Claude Code, on **your** Claude subscription, started by EAEH in the background |
| Checks schemas, BRD coverage, standards and permissions | EAEH |
| **Approves, requests changes or rejects** each document | A person: you, as the approver. No AI and no author can approve. |
| Stores every version, the audit log and traceability; writes the approved documents to `docs/architecture/` in your project | EAEH |

You work in either of two ways, and they share the same data:
* **CLI:** the `eaeh` command in a terminal. It does everything.
* **Portal:** a local web UI to watch progress and review and decide documents. It runs on your computer only and has no login.

Roles, all held by you in a single-person setup: **admin** (sets EAEH up), **developer** (runs workflows), **steward** (owns the BRD and knowledge), **approver** (decides).

---

## 2. Get and build EAEH

### 2.1 Prerequisites

| Tool | Check (📁 any folder) | Needed for |
|---|---|---|
| git | `git --version` | cloning |
| Rust (rustup) | `cargo --version` | building `eaeh` (the toolchain version is pinned in the repository) |
| Node.js ≥ 22.12 | `node --version` | building the portal UI |
| Claude Code, signed in with your **Claude subscription** | `claude auth status` → `"authMethod": "claude.ai"` | AI work (not needed for the demo in section 5) |

If Claude Code isn't signed in, run `claude`, type `/login` and sign in with your Claude account. **Do not use an API key**; EAEH refuses that.

### 2.2 Clone and build

📁 **Any folder** (the clone becomes your EAEH folder):

```bash
mkdir -p ~/repo && cd ~/repo
git clone https://github.com/udithad/enterprise-architecture-engineering-harness.git
cd enterprise-architecture-engineering-harness
cargo build -p eaeh-cli                                   # → target/debug/eaeh
npm --prefix portal ci && npm --prefix portal run build    # → portal/dist (web UI)
```

### 2.3 Make `eaeh` available in every terminal

📁 **EAEH folder**, run it in the same terminal as 2.2 (it writes this folder's path into your shell profile):

```bash
echo "export PATH=\"$PWD/target/debug:\$PATH\"" >> ~/.zshrc        # bash: ~/.bashrc
echo "export EAEH_CONFIG=\"$PWD/config/eaeh.toml\"" >> ~/.zshrc
source ~/.zshrc
```

`EAEH_CONFIG` selects your **installation**: its configuration and its data directory (`.eaeh-data` in the EAEH folder). The CLI, the portal and the background worker must all use the same one.

If an older `eaeh` exists (`which -a eaeh` prints more than one line), remove the others, for example with `cargo uninstall eaeh-cli`.

---

## 3. Set up EAEH

📁 **Any folder**, once:

```bash
eaeh init                                                    # database and definitions
eaeh admin bootstrap --tenant acme --project platform --admin-id admin --token-file ~/admin.token
eaeh auth login --profile admin --token-file ~/admin.token && rm ~/admin.token
```

`acme` is your organisation's name in EAEH; choose any. `admin bootstrap` works only once per installation.

**Check** (open a new terminal; 📁 any folder):

```bash
eaeh health          # OK
eaeh auth status     # profile "admin" … valid
```

The shipped `config/eaeh.toml` is ready to use. It runs in `external_ai` mode: Claude Code does the AI work, automatically, through a background worker.

---

## 4. Start the portal

📁 **Project folder** (or any folder), in **your own terminal window**, not in an AI assistant's shell:

```bash
eaeh portal --as developer --approver approver --steward steward --open
```

* `--open` opens your browser with a one-time link: `http://127.0.0.1:8090/?launch=…`. **Keep the terminal open**; Ctrl-C stops the portal. Each start creates a new link and secret, and old links stop working.
* `--as` is who the portal *views* as. `--approver` is who it *decides* as (documents and skills). `--steward` is who it *registers skill files* as. Leave out `--approver` for a read-only portal, and `--steward` if you don't register skills in the portal.
* The profiles `developer`, `approver` and `steward` exist after your first `project init` (6.2). Before that, use `eaeh portal --as admin --open`.
* The terminal shows `data dir …`, and the portal shows it at the top of every page. It must be your installation (`…/enterprise-architecture-engineering-harness/.eaeh-data`).

| Page | Shows |
|---|---|
| Dashboard | Work in progress (AI or approval), pending approvals, recent activity |
| Projects | Every project: status (Ready, In progress, Waiting for approval, Blocked, Completed, Failed), current stage, workspace, stack |
| Workflows | Each run's lifecycle, stages, governance checks, and documents written to the workspace |
| Approvals | Review a document or a skill → **Approve**, **Request changes** or **Reject** (the last two need a comment) → confirm |
| Skills | Every skill file and its layers (stages) and status; **Register skill files**; Review links |

The portal refreshes by itself (about every 10 seconds). Mandatory standards are approved with the CLI only.

---

## 5. Try the demo (no AI, no subscription)

The demo runs a **separate, throw-away installation** (`target/demo-portal`) in *deterministic* mode. Recorded answers are replayed: nothing is AI-generated and nothing is charged. It uses the sample BRD of a small Spring Boot API.

📁 **EAEH folder**:

```bash
scripts/demo-portal.sh prepare          # installation, sample project, skills registered
scripts/demo-portal.sh approve-skills   # you approve each skill (type y)
scripts/demo-portal.sh start            # starts the design run, then the demo portal
```

* **Portal:** open the printed `http://127.0.0.1:8190/?launch=…` link. Port **8190** belongs to the demo; it is labelled **DEMO**. Go to **Approvals → Review**, then approve the Requirements, the HLD and the LLD in turn.
* **CLI:** none needed; the demo is driven from the portal.
* **Stop:** Ctrl-C. Restart on the same data with `scripts/demo-portal.sh portal`. Delete everything with `rm -rf target/demo-portal`.

> The demo never shows your real projects, and your real portal (port 8090) never shows the demo.

---

## 6. Set up a new project

These steps use the **sample project**, a small Spring Boot REST API with a BRD that ships with EAEH. For your own project, use your repository and your BRD, and change `--name`, `--description` and the stack options.

### 6.1 Create the project folder

📁 **Any folder**:

```bash
mkdir -p ~/repo/simple-springboot-api && cd ~/repo/simple-springboot-api
git init
cp "$(dirname "$EAEH_CONFIG")/../examples/simple-springboot-api/BRD-Simple-Spring-Boot-API.md" .
cp "$(dirname "$EAEH_CONFIG")/../examples/simple-springboot-api/PROJECT-BOOTSTRAP.md" .
```

### 6.2 Register the project (one command)

📁 **Project folder**:

```bash
eaeh project init --name simple-springboot-api \
  --description "Simple REST API for local development" \
  --language "Java 21" --framework "Spring Boot 3.x" --build-tool Maven \
  --constraint "Simple layered architecture: controllers, services, models" \
  --constraint "Products in memory; no database" \
  --constraint "No authentication, authorization or Spring Security" \
  --brd BRD-Simple-Spring-Boot-API.md \
  --knowledge PROJECT-BOOTSTRAP.md \
  --developer dev --steward steward --approver approver \
  --build-verification deferred \
  --store-profiles
```

This one command:
* registers the project and stores the BRD;
* creates the people `dev`, `steward` and `approver`, and stores their logins as the profiles `developer`, `steward` and `approver`;
* writes `.eaeh/project.toml`, `CLAUDE.md` and `.mcp.json`. They contain no secrets and are safe to commit.

It is safe to run again.

It ends with **NOT READY** (exit code 7) the first time. That is expected, because the skills need approval.

* **Portal:** the project now appears under **Projects** with status **Ready**.

### 6.3 Approve the skills (once per installation)

📁 **Project folder**:

```bash
eaeh approval review --as approver               # read every pending skill
eaeh approval review --as approver --all --yes   # approve them
eaeh project readiness                           # must say READY
```

* **Portal:** open **Skills**. Each pending skill has a **Review** link, which shows the whole skill (layers, tools, instructions, digest); approve it there. **Approvals** lists the same requests. The project page shows the readiness result.

Later projects reuse the approved skills and are usually READY straight away.

---

## 7. Run the design and approve

### 7.1 Start

📁 **Project folder**:

```bash
eaeh workflow run design --yes
eaeh workflow watch            # optional: live progress (Ctrl-C stops watching, not the work)
```

A background worker runs Claude Code for each AI step. Expect a few minutes per document. You get a desktop notification when a document needs a decision.

* **Portal:** **Dashboard** or **Projects** shows the run as *In progress* with the AI step, then *Waiting for approval*.

### 7.2 Review and decide (three times: Requirements, HLD, LLD)

| | Do |
|---|---|
| **CLI** (📁 project folder) | `eaeh review --as approver`. This shows the document, then type **a** (approve), **c** (request changes, with a comment) or **r** (reject, with a comment). |
| **Portal** | **Approvals → Review**. Read the document and its checks, choose a decision, comment if needed, confirm. |

* **Approve:** the next stage starts automatically.
* **Request changes:** the AI revises the document and you review the new version.
* **Reject:** the run ends.

### 7.3 Results

Each approved document is written to your project. Every file is checked against the approved version:

```
docs/architecture/requirements/   requirements.md, requirements.json, analysis.json
docs/architecture/hld/            hld.md, hld.json, adr/*.md, diagrams/*.mmd
docs/architecture/lld/            lld.md, lld.json, diagrams/*.mmd
.eaeh/artifacts.json              version, hash, approver and sources of each document
```

📁 **Project folder**:

```bash
eaeh workflow status                 # stages, documents, next step
eaeh artifact status                 # each document: version, approver, folder, VERIFIED?
eaeh trace --id REQ-F-001 --depth 3  # BRD → requirement → HLD → LLD
git add docs/architecture .eaeh && git commit -m "Approved design"   # when you are ready
```

* **Portal:** the workflow page has a **Documents in the project workspace** card showing each document as *verified*, *modified locally*, *missing* or *awaiting approval*.

The `review/` folder holds **unapproved** review copies from `eaeh review`; keep it out of Git.

### 7.4 Next project

Repeat sections 6 and 7 in the new project's folder, with a different `--name`. The people and skills already exist. One portal shows every project of the installation.

---

## 8. Day-to-day: CLI and portal side by side

| You want to | CLI (📁 project folder unless noted) | Portal |
|---|---|---|
| See all projects | `eaeh project list` (any folder) | **Projects** |
| See a project's state | `eaeh workflow status` | **Projects → project** |
| Follow a run | `eaeh workflow watch` | **Workflows → run** (auto-refresh) |
| Decide a document | `eaeh review --as approver` | **Approvals → Review** |
| Register skill files | `eaeh skill sync` | **Skills → Register skill files** (portal started with `--steward`) |
| Approve skills | `eaeh approval review --as approver` | **Skills → Review** (or **Approvals**) |
| Approve mandatory standards | `eaeh approval review --as approver` | – (CLI only) |
| Check readiness | `eaeh project readiness` | **Projects → project** (Readiness) |
| Check the documents in the repository | `eaeh artifact status` | **Workflows → run** (Documents card) |
| Write missing approved documents | `eaeh artifact sync` | – (CLI only) |
| Check the AI worker | `eaeh worker status` / `eaeh worker log` | Dashboard (*Waiting for AI*) |
| Audit trail | `eaeh audit list` / `eaeh audit verify` (any folder) | **Activity** |

---

## 9. Troubleshooting

| You see | Do |
|---|---|
| `eaeh: command not found` / config not found | Repeat 2.3 and open a new terminal |
| `no active project` | `cd` into the project folder |
| `NOT READY` | Run the fix printed under each `BLOCK` line (often 6.3) |
| Nothing happens after `workflow run` | `eaeh worker status`, then `eaeh worker log` |
| Task `blocked: usage_limit` / `auth_unavailable` | Wait for the reset / `claude` → `/login`; then `eaeh worker retry <TASK-ID>` |
| `FAILED at stage …` | Fix the cause; start a new run: `eaeh workflow run design --yes` |
| `BLOCKED at '<gate>'` / documents missing in `docs/architecture/` | `eaeh artifact status` explains why; fix it, then `eaeh artifact sync` |
| `not written – existing files would be overwritten` | Move or commit your own file there, then `eaeh artifact sync` (or `--replace-modified` to replace it with the approved version) |
| Portal: `ERR_CONNECTION_REFUSED` | The portal isn't running. Start it (section 4) and keep its terminal open |
| Portal: no `launch=` secret | The link is printed once in the portal's terminal. If it's gone, Ctrl-C and start again with `--open` |
| Portal: a project is missing | The portal uses another installation. Compare the data dir at the top of the portal with `eaeh project list`; the DEMO (port 8190) is separate |
| `port 8090 is already in use` | Another portal is running: `lsof -nP -iTCP:8090 -sTCP:LISTEN`, stop it, or use `--bind 127.0.0.1:<port>` |
| `approval decisions require an explicit approver identity` | Add `--as approver` |

Every error prints a `hint` with the fix. Exit codes: 3 credentials, 4 permission, 5 not found, 6 integrity, 7 not ready, 8 documents not written.

---

## 10. Skills: write, register, approve

A **skill** is reusable instruction text that EAEH adds to the AI's task at specific **layers** (workflow stages). For example: rules for diagrams in every HLD and LLD. A skill never grants permissions, and it is applied only after a person approves it.

| Layer (stage) | Task kind to put in `task_kinds` |
|---|---|
| Requirements: analysis / review | `requirements-analysis` / `requirements-review` |
| HLD | `hld-design` |
| LLD | `lld-design` |
| Implementation plan / code and tests | `implementation-planning` / `implementation` |

### 10.1 Write a skill: the `SKILL.md` format

EAEH reads the standard **`SKILL.md`** used by community skill repositories: a YAML header between `---` lines, then Markdown instructions. You can copy a community skill folder into `config/skills/` unchanged (📁 EAEH folder). It applies to all projects of this installation. Example: the supplied `config/skills/architecture-designer/`:

```markdown
---
name: architecture-designer            # required: lowercase id
description: Use when designing new high-level system architecture …   # required
license: MIT                           # optional
metadata:                              # optional, any keys
  version: "1.1.1"                     # the skill version (default 1.0.0)
  scope: design                        # → applied at HLD and LLD (see below)
  author: https://github.com/Jeffallan
---

# Architecture Designer

Senior software architect specializing in system design …
```

* **Required:** `name` and `description`. Every other property is kept, shown to reviewers and covered by the approval. None is rejected.
* **The instructions:** everything after the closing `---` is the instruction text the AI receives.
* **Layers** (where EAEH applies the skill):

  | Source | Rule |
  |---|---|
  | **Explicit** | `metadata.eaeh-task-kinds: hld-design, lld-design`, or `task_kinds:` |
  | **Inferred** from `metadata.scope` | `design`, `architecture` → HLD and LLD<br>`requirements`, `analysis` → requirements<br>`planning` → implementation plan<br>`implementation`, `code`, `development` → implementation |
  | **Otherwise** | No layer: the skill can be approved but isn't applied anywhere |

  The portal shows the layers and how they were determined before you approve.
* **Older formats keep working:** EAEH's TOML header (`+++` … `+++`, e.g. `config/skills/architecture-diagrams.md`) and JSON skill files (`*.json`).
* **Changing a skill:** for a skill folder, edit any file and a new version is registered automatically (10.2). For a single file, raise the version.

### 10.2 Or: a skill folder (with reference files)

For a skill with supporting material, copy a **folder** into `config/skills/`. It needs a `SKILL.md` (10.1); any other files and subfolders are optional:

```
config/skills/api-error-handling/          # example shipped with EAEH (LLD layer)
├── SKILL.md                               # header (--- YAML ---, or +++ TOML +++) + main instructions
├── references/error-catalog.md            # reference documents …
├── references/http/status-mapping.md      # … nested folders are fine
├── examples/error-response.json
├── examples/config.yaml
└── scripts/validate_errors.py             # shown to reviewers; never run, never sent to the AI
```

* **What the AI receives once approved:** the `SKILL.md` text and the text files (Markdown, JSON, YAML, TOML, text), up to 64 KiB per skill. Scripts and binary files are listed only.
* **The approval covers every file.** If you change, add or remove **any** file after approval, EAEH registers a **new version automatically**, for example `1.0.1`, as *waiting for approval*. The approved version stays in use until someone approves the new one.
* **Limits:** 2 MiB per folder, 512 KiB per file, 500 files. Symbolic links are refused.

### 10.3 Register

| | Do |
|---|---|
| **Skill folders** | Nothing to do: EAEH finds new and changed folders by itself. The running portal checks about every 30 seconds, and the CLI checks on `project init`, `skill sync`, `skill pending` and `project readiness`. **Portal:** **Skills → Scan now** checks immediately. |
| **Single files, CLI** (📁 project folder, as the steward) | `eaeh skill sync`. It registers every skill file the project's layers use, as *waiting for approval*. `eaeh project init` does the same. |
| **Single files, portal** (started with `--steward steward`) | **Skills → Register skill files** does the same for every file in the skills folder. |

### 10.4 Review and approve

| | Do |
|---|---|
| **CLI** (📁 project folder) | `eaeh approval review --as approver` (read), then `eaeh approval review --as approver <ITEM>` or `--all --yes`, or `<ITEM> --reject --comments "<REASON>"` |
| **Portal** | **Skills**, then the **files** link of a version. This opens the skill viewer:<br>• the folder tree with every file;<br>• Markdown rendered; JSON and YAML formatted; code with line numbers;<br>• each file's hash;<br>• each file marked *added* or *modified* compared with the version in use;<br>• the approval history.<br>Then **Review and decide**: **Approve**, or **Reject** with a comment (required), and confirm. |

Skills are approved or rejected; *request changes* is only for workflow documents. The person who registered a skill can't approve it. Once a skill is approved, every matching layer of the **next** stages uses it. Each document records which skills produced it.

### 10.5 Check

```bash
eaeh skill pending     # 📁 project folder: skills per layer and their state
eaeh skill list        # 📁 any folder: every registered skill version
```

* **Portal:** the **Skills** page shows the status of each skill:
  * *Not registered*;
  * *Waiting for approval*;
  * *Approved · applied*, with an **in force** label on the version the AI uses;
  * *Rejected*;
  * *File changed – bump version*: a single file was edited without raising `version`. Skill folders get a new version automatically instead.

A pending layer skill shows as a warning in `eaeh project readiness` and doesn't block the project. The skills the agents themselves require do block it.

---

## 11. Reference

**Execution modes** (`[execution] mode` in `config/eaeh.toml`; 📁 EAEH folder):

| Mode | AI by | Cost |
|---|---|---|
| `external_ai` (default) | Claude Code through the background worker | Your Claude subscription and its limits |
| `deterministic` | Recorded answers, **not AI** (demo, tests) | None |
| `direct_provider` | EAEH calls the Anthropic API | A separate API key and bill (`EAEH_SECRET_ANTHROPIC_API_KEY`) |

**Working without the worker:** `eaeh workflow run design --yes --no-worker`, then in Claude Code or VS Code Copilot (in the project folder) ask *"Do the EAEH task that is waiting"*. The `eaeh` MCP server is already configured by `project init`. The assistant can never approve.

**More people (team setup)**, 📁 any folder as admin:

```bash
eaeh principal create --id <PERSON-ID> --display-name "<NAME>" --roles approver --projects all
eaeh token issue --principal <PERSON-ID> --label "<LABEL>" --token-file ~/<PERSON-ID>.token
```

The person then runs, 📁 any folder on their machine: `eaeh auth login --profile <PERSON-ID> --token-file <FILE>`. They act with `--as <PERSON-ID>`. In `project init`, `--handover-dir <FOLDER>` replaces `--store-profiles`.

**Implementation and tests (optional, after an approved design):**
1. 📁 **Project folder:** `eaeh workflow run deliver --yes`.
2. Approve the implementation plan, then the code bundle.
3. Copy the approved code into your repository: `eaeh artifact export <BUNDLE-ID> --out .`

**Environment variables:** `EAEH_CONFIG` (installation), `EAEH_HOME` (where profiles are stored), `EAEH_PROFILE` (default `--as`).

**Every command** has `--help`. Global options: `--config <FILE>`, `--as <PROFILE>`, `--json`.

**Deeper documentation:**
* [Governance Portal](../implementation/EAEH-GOVERNANCE-PORTAL.md)
* [Approved documents in the workspace](../implementation/EAEH-ARTIFACT-MATERIALIZATION.md)
* [Architecture decisions](../architecture/adr/)
