# EAEH User Guide

From `git clone` to approved Requirements, HLD and LLD in your own project. Follow the sections in order; each takes a few minutes.

| # | Section | How often |
|---|---|---|
| 1 | [What EAEH does](#1-what-eaeh-does) | Read once |
| 2 | [Get and build EAEH](#2-get-and-build-eaeh) | Once per computer |
| 3 | [Set up EAEH (`eaeh setup`)](#3-set-up-eaeh) | Once per computer; again to repair or reset |
| 4 | [The portal](#4-start-the-portal) | Started by setup; stop and restart here |
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

One command sets up and verifies the whole local environment, then starts the portal.

📁 **Any folder**:

```bash
eaeh setup --open
```

`--open` also opens your browser. The command is **safe to run again**: it never recreates valid people or overwrites working logins. What it does, in order:

1. creates the data directory and database (or checks them);
2. creates the tenant `local` and the project `default` (first run only; change with `--tenant` / `--project`);
3. creates the administrator and stores the login as the profile `admin` (the token is never printed);
4. creates and **verifies** the standard people and profiles (table below);
5. gives each person access to **every project of the tenant** (the portal reads as these people);
6. checks the workflow definitions and agents;
7. discovers the skills in `config/skills/` and registers them as **pending approval** (never approved automatically);
8. checks the AI client, the worker and the audit chain;
9. starts the portal in the background and prints its address.

| Profile (`--as …`) | Person (principal) | Role | Used for |
|---|---|---|---|
| `admin` | `admin` | `admin` | people, logins, projects |
| `developer` | `dev` | `operator` | starting and operating workflows |
| `approver` | `approver` | `approver` | approval decisions |
| `steward` | `steward` | `knowledge_steward` | skills and knowledge |

A profile counts as valid only when its token **authenticates as the intended person with exactly the intended role**. A valid token of the wrong person (for example `developer` holding the admin's token) is reported as `IDENTITY MISMATCH` and is never used. Administrators cannot start workflows or approve; developers, approvers and stewards never get admin rights.

**Example result** (shortened):

```text
EAEH Setup: SUCCESS (with warnings)
  [ok      ] Database: healthy (7 migrations applied, …/eaeh.db)
  [repaired] admin: administrator principal 'admin' created; credential stored as profile 'admin' (not displayed)
  [ok      ] admin: valid – profile 'admin' → principal 'admin' (admin)
  [repaired] developer: principal 'dev' created with role operator
  …
  [repaired] Skills: 54 discovered, 54 registered, 0 changed, 0 already registered, 0 invalid; 54 awaiting human approval
  [WARNING ] Skill approval: 54 skill version(s) need a human approver; unapproved skills are NOT available to agents or workflows
  [ok      ] Governance Portal: running (started), pid 36362, http://127.0.0.1:8090

Portal URL: http://127.0.0.1:8090
Launch link (contains a one-time secret – do not share): http://127.0.0.1:8090/?launch=…
```

Status words: `ok` (verified), `repaired` (setup fixed it), `WARNING` (not blocking: a human decision or optional part), `FAILED` (mandatory part not verified: the portal is **not** started and the exit code is 7), `skipped`. Every `FAILED` and `WARNING` line comes with the exact command under **What to do**. Setup never reports success while a mandatory part is unverified.

### 3.1 Options

| Command | What it does |
|---|---|
| `eaeh setup` | Set up or repair, then start the portal |
| `eaeh setup --open` | …and open it in your browser |
| `eaeh setup --check` | Diagnose only (exit 7 if something mandatory is not ready). Changes no identity, credential or skill and starts nothing |
| `eaeh setup --repair` | Also fix a profile that maps to the **wrong person** or belongs to another installation, and re-enable a disabled standard person. The credential file is backed up first |
| `eaeh setup --reset` | Back up, then start from a clean database (3.3) |
| `eaeh setup --restore <BACKUP-DIR>` | Put a backup back (3.3) |
| `eaeh setup --no-portal` | Everything except starting the portal |
| `eaeh setup --stop-portal` | Stop the background portal |
| `--bind 127.0.0.1:<PORT>` | Portal address (loopback only; default 8090) |
| `--tenant`, `--project`, `--ttl-days` | First-run names; token lifetime (default 30 days) |
| `--json` (global) | Machine-readable report |

Without options, a repeat run repairs what is safe: a missing person, missing project access, a missing, expired or revoked login. It does **not** replace a login that belongs to a different person; that needs `--repair`. A person's roles are never changed, with or without `--repair`.

### 3.2 Running setup again

Run `eaeh setup` whenever something seems off (a login expired, a profile was overwritten, you added skill folders). Valid people, valid logins and skills already registered are left exactly as they are. In a terminal, on an already configured installation, you are asked to choose:

* **A) Continue with the existing database and repair configuration** (default).
* **B) Reset and configure EAEH from the beginning** (3.3).

### 3.3 Reset, backup and restore

Choose this for a completely fresh installation:

```bash
eaeh setup --reset
```

It prints what you lose (projects, workflows, approvals, audit records and knowledge in the database) and asks you to type `RESET EAEH`. Without a terminal, pass `--confirm-reset "RESET EAEH"`. Anything else changes nothing.

Before touching the database, setup writes a **verified backup** to `~/.config/eaeh/backups/<UTC-TIMESTAMP>/` (or `$EAEH_HOME/backups/…`), folder mode `0700`, files `0600`:

* `eaeh.db`: a consistent copy, checked with SQLite's integrity check;
* `credentials.json`: your login profiles (they contain tokens: protect the folder);
* `eaeh.toml` and `MANIFEST.json` (file hashes).

Then only the database files are deleted. Your skills, configuration, source-code workspaces, `workspaces/` and `artifacts/` folders are never deleted. The logins of this installation are replaced by newly issued ones. A running background portal is stopped first. Do not reset while `eaeh serve` or the worker is running.

To go back:

```bash
eaeh setup --restore ~/.config/eaeh/backups/<UTC-TIMESTAMP> --confirm-reset "RESET EAEH"
```

Restore verifies the backup's hashes (a modified backup is refused), backs up the current state first, then restores the database and this installation's login profiles, and runs the normal checks. A reset that was interrupted before the new setup finished leaves an empty database: just run `eaeh setup` again.

### 3.4 Lost or broken administrator login

Repairs are performed **as the administrator** through the normal services; setup never bypasses authorisation and never invents credentials. If the profile `admin` is missing, expired, revoked or maps to the wrong person, setup stops before changing anything and tells you:

* you still have an administrator token: `eaeh auth login --profile admin --token-file <FILE>`, then `eaeh setup`;
* the token is lost: the only recovery is `eaeh setup --reset` (backed up first).

### 3.5 Check

📁 Any folder:

```bash
eaeh setup --check      # everything verified?
eaeh auth status        # profiles: person, roles, expiry, valid
eaeh health             # OK
```

The shipped `config/eaeh.toml` runs in `external_ai` mode: Claude Code does the AI work, automatically, through a background worker. Setup reports whether the `claude` client is installed and logged in with a subscription; it cannot prove a working sign-in without making a model call, which `external_ai` mode forbids.

---

## 4. Start the portal

`eaeh setup` starts the portal for you (section 3) in the background, using the profiles above: viewing as `admin`, deciding as `approver`, operating as `developer`, registering skills as `steward`, registering projects as `admin`. The address and a one-time launch link are printed. If the portal already runs for this installation, setup reuses it and prints its link again.

* **Stop:** `eaeh setup --stop-portal`.
* **Restart:** `eaeh setup` (or `eaeh setup --open`).
* **Port in use:** setup reports `port conflict`; find the owner with `lsof -nP -iTCP:8090 -sTCP:LISTEN`, stop it, or use `eaeh setup --bind 127.0.0.1:<PORT>`.
* **Web UI missing** (`portal/dist` not built): setup says so and does not start the portal; run `npm --prefix portal ci && npm --prefix portal run build`, then `eaeh setup`.
* The portal's log is `~/.config/eaeh/portal/portal-<id>.log` (mode `0600`).

You can still run the portal yourself in the foreground, for example with other identities, in **your own terminal window**, not in an AI assistant's shell:

```bash
eaeh portal --as developer --approver approver --steward steward \
  --operator developer --admin admin --open
```

* `--open` opens your browser with a one-time link: `http://127.0.0.1:8090/?launch=…`. **Keep the terminal open**; Ctrl-C stops the portal. Each start creates a new link and secret, and old links stop working.
* `--as` is who the portal *views* as.
* `--approver` is who it *decides* as (documents and skills).
* `--steward` is who it *registers skill files* as.
* `--operator` is who it *starts, cancels, resumes, restarts and retries* workflows as. Use the developer profile.
* `--admin` is who it *registers existing projects* as.

Leave out any identity you don't need, and the matching buttons disappear. The backend still checks every action against that identity's role, and records it in the audit trail.

* `--workspace-root <folder>` (repeatable) sets which folders **Register existing project** may browse. The default is the folder that contains the EAEH clone (for example `~/repo`).
* The terminal shows `data dir …`, and the portal shows it at the top of every page. It must be your installation (`…/enterprise-architecture-engineering-harness/.eaeh-data`).

| Page | Shows |
|---|---|
| Dashboard | Work in progress (AI or approval), pending approvals, recent activity |
| Projects | Every project: status (Ready, In progress, Waiting for approval, Blocked, Completed, Failed), current stage, workspace, stack |
| Workflows | Each run's lifecycle, stages, governance checks, and documents written to the workspace |
| Approvals | Review a document or a skill → **Approve**, **Request changes** or **Reject** (the last two need a comment) → confirm |
| Skills | Every skill file and its layers (stages) and status; **Register skill files**; Review links |
| Tasks | Every task across projects; filters by project, workflow, stage, status and date. Each task opens its page: input, agent and skills, dependencies, hand-off, attempts, output, validation, errors, **Retry** |
| Knowledge & agents | Registered knowledge documents and sources, agents and tools (read-only) |
| Execution | Workers, AI hand-offs, execution attempts and failures; AI-client integration status |
| Settings | The identities the portal uses, workspace roots, health, audit-chain check |

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

**Or, in the portal** (started with `--admin admin`):
1. Go to **Projects → Register existing project**.
2. Choose the folder. EAEH detects the language, framework, build tool, Git branch, BRD, README, architecture documents, ADRs and standards. Nothing is changed yet.
3. Check and edit: name, description, stack, constraints, BRD, knowledge documents, standards, developer, steward, approvers.
4. Select **Register project**.

This runs the same service as `eaeh project init`. It writes the same files. It never copies or moves your source code. New people's logins go to this computer's credential store, never to the browser. Afterwards, restart the portal with `--operator developer` to start workflows.

A folder outside the workspace roots, a folder that isn't a project, a folder that is already registered, an existing project name, and document paths outside the folder are all refused with a message.

Either way, the result is **NOT READY** (exit code 7) the first time. That is expected, because the skills need approval.

* **Portal:** the project now appears under **Projects** with status **Ready**.

### 6.3 Approve the skills (once per installation)

`eaeh setup` has already registered the skills in `config/skills/` as pending; `project init` registers the ones this project's workflows need. Approving is always a human decision.

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
* **Or start it in the portal** (started with `--operator developer`):
  1. Go to **Projects → project → Start workflow**.
  2. Choose the workflow. The list and the stages come from EAEH, not from the page.
  3. Check the execution plan: stages, agents, skills, approvals, input and readiness.
  4. Select **Start**. The portal also starts the background worker if AI work is waiting.

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

### 7.4 Resume, retry and restart

A run can be continued, retried or re-run from an earlier stage. A restart never rewinds the old run. It creates a **new run linked to it**:
* stages before the restart point are reused as they are (same document versions and valid approvals);
* the restart point and everything after it run again and produce **new versions**, which need new approvals;
* approved documents are never overwritten, and the old run keeps its history;
* if the old run was still active, it is cancelled as *superseded*.

| You want to | CLI (📁 project folder) | Portal (started with `--operator`) |
|---|---|---|
| Continue an interrupted run (after a restart of EAEH, or while it waits for AI) | `eaeh workflow resume <RUN-ID>` | **Workflow → Resume** |
| Retry a failed stage | `eaeh workflow restart <RUN-ID>` (default: the failed stage) | **Workflow → Retry failed stage…** |
| Re-run from an earlier stage (requirements or architecture changed) | `eaeh workflow restart <RUN-ID> --from hld` | **Workflow → Restart from stage…** |
| Re-run with a new BRD version | `eaeh workflow restart <RUN-ID> --from brd --input '{"brd":"<citation>@v2"}'` | Same dialog, **New input** |
| Retry an AI hand-off a worker gave up on | `eaeh worker retry <TASK-ID>` | **Task → Retry AI hand-off** |
| Cancel | `eaeh workflow cancel <RUN-ID>` | **Workflow → Cancel** |

Before anything runs, both show a **preview**:
* the restart point;
* what is reused and what is re-executed;
* the current document versions;
* which approvals stay valid and which must be repeated;
* the effect on the workspace documents.

To confirm, the CLI needs `--yes --reason "<why>"`, and the portal needs **Confirm restart** with a reason. Only stages of the run's workflow can be chosen, and never one that would skip an approval that hasn't been given. The workflow page shows **Lineage** (*restarted from* / *superseded by*).

### 7.5 Next project

Repeat sections 6 and 7 in the new project's folder, with a different `--name`. The people and skills already exist. One portal shows every project of the installation.

---

## 8. Day-to-day: CLI and portal side by side

| You want to | CLI (📁 project folder unless noted) | Portal |
|---|---|---|
| See all projects | `eaeh project list` (any folder) | **Projects** |
| Register an existing project | `eaeh project init …` | **Projects → Register existing project** (`--admin`) |
| Start a workflow | `eaeh workflow run design --yes` | **Projects → project → Start workflow** (`--operator`) |
| Resume, restart, retry, cancel | `eaeh workflow resume / restart / cancel`, `eaeh worker retry` | **Workflow** and **Task** pages (`--operator`) |
| See tasks | `eaeh task list` | **Tasks** (filters) → task page |
| See a project's state | `eaeh workflow status` | **Projects → project** |
| Follow a run | `eaeh workflow watch` | **Workflows → run** (auto-refresh) |
| Decide a document | `eaeh review --as approver` | **Approvals → Review** |
| Register skill files | `eaeh skill sync` | **Skills → Register skill files** (portal started with `--steward`) |
| Approve skills | `eaeh approval review --as approver` | **Skills → Review** (or **Approvals**) |
| Approve mandatory standards | `eaeh approval review --as approver` | – (CLI only) |
| Check readiness | `eaeh project readiness` | **Projects → project** (Readiness) |
| Check the documents in the repository | `eaeh artifact status` | **Workflows → run** (Documents card) |
| Write missing approved documents | `eaeh artifact sync` | – (CLI only) |
| Check the AI worker | `eaeh worker status` / `eaeh worker log` | **Execution** (workers, attempts, redacted failure details); full logs: CLI only |
| Audit trail | `eaeh audit list` / `eaeh audit verify` (any folder) | **Activity**; **Settings** (chain check) |

Tokens, people, plugins, knowledge ingestion and deletion, standards approval, worker start and stop, and artifact export stay with the CLI. See `docs/implementation/EAEH-PORTAL-CLI-PARITY.md` for every command and the reason.

> The portal is for **local development only**. It has no login, it listens on 127.0.0.1, it needs the launch secret, and it acts only as the identities you start it with. Don't expose it to other users or machines.

---

### 8.1 How the AI receives its task (the hand-off protocol)

The AI client in the background worker has **no file or shell tools**, and a single tool response above a client-side limit (about 100 KB in Claude Code) is diverted to a file it cannot open. EAEH therefore never sends a task in one piece. The worker's prompt makes the AI follow this protocol; GitHub Copilot or another MCP client uses the same tools:

| Step | Tool | What happens |
|---|---|---|
| 1 | `eaeh_task_get` | Returns the **manifest**: `package_hash` (the version), the sections with size, checksum and class (**mandatory** or reference), warnings and the protocol. A small task is included whole under `package_sections` |
| 2 | `eaeh_task_section` | For each mandatory section not included whole: read from `offset` 0, repeat with `next_offset` until `done` is true (chunks of 16 KB, never cut; pass `package_hash`). Mandatory sections: `overview`, `instructions`, `rules`, `output_schema`, `output_outline`, `output_example`, `skill_contracts`, `allowed_tools`, `citable`, `input`. `data` is untrusted reference material |
| 3 | `eaeh_task_ack` | Confirms every mandatory section was retrieved completely. EAEH checks its own delivery record (and the checksums the AI sends, if any). Until then the next three tools refuse |
| 4 | `eaeh_task_tool`, `eaeh_task_blob` | More information through the agent's allowed tools (for example `knowledge.read` for the BRD). A result above 24 KB comes back `paged`: read the other pages with `eaeh_task_blob` |
| 5 | `eaeh_task_validate`, `eaeh_task_submit` | Free check, then the result. Only a validated result continues the workflow, then a human approves at the gate |

The package is stored once per task and never changes, so a restarted worker delivers the same bytes. Repeated content is sent once (`input` values that repeat another key show `{"$same_as": "<key>"}`; `data` shows `[identical to the 'input' section]`). A refinement task of about 235 KB is delivered as about 50 KB. A task above 16 MiB is refused when it is created, naming its largest parts. Nothing is truncated.

**Authorization.** Each attempt of the worker receives a **capability**: read of exactly its own task context (tenant, project, workflow, task, package version). It lasts at most as long as the attempt (the task timeout, no margin), stops working as soon as the attempt is no longer running, and is revoked when the attempt ends or the task is retried. With it, reading the context needs no interactive approval, which a worker session cannot give. It does not allow anything else: tools, validation and submission keep their normal authorization, and approvals stay human decisions. Every read, and every refusal, is in the audit log (`external_task.context_read`, `external_task.context_denied`, `external_task.context_acknowledged`). The worker lets Claude Code use only the task-scoped `eaeh` MCP server (nothing else), and before each attempt checks that this server is the same build as the worker.

**Failure names** (`eaeh worker history`, portal *Execution* and *Task* pages, audit log):

| Name | Meaning | Uses an attempt? | Retried? |
|---|---|---|---|
| `context_delivery_failed` | The AI never acknowledged its task: never asked, stopped part-way, response too large for it | No | Up to `max_context_attempts` (3), growing wait, then `blocked` |
| `context_access_denied` | The AI was refused its context: no/expired/revoked capability, a tool denied for lack of approval, or worker and MCP server are different builds | No | **No** (needs a fix) |
| `context_integrity_failed` | The AI worked on another package version or its checksums differ | No | Like delivery failures |
| `no_submission` | The AI had its task (acknowledged) but ended without a result | Yes (`max_attempts`) | Yes |
| `ai_execution_failed` | The AI had its task, then the client crashed or reported a tool error | Yes | Yes |
| `submission_failed` | The AI submitted, but EAEH could not store or process it (infrastructure) | Yes | Yes |
| `result_validation_failed` | Results were rejected until the correction attempts ran out | Correction attempts | No |
| `task_cancelled` | Stopped on request (workflow cancelled, worker stopped) | No | No |

`eaeh worker history` shows, per attempt, the package size, delivery state (`pending`, `partial`, `complete`, `acknowledged`, `not_requested`), sections retrieved, acknowledgement, access denials and the remedy.

**Recovery.** After fixing the cause:

```bash
eaeh task list                                   # find the task; shows its workflow and stage
eaeh worker retry <TASK-ID> --workflow <WORKFLOW-ID>
```

It checks that the task belongs to that workflow and that the workflow still waits at that stage, and prints the chain. It keeps the package version, every submission and every accepted result, creates no new task and resets only the delivery and dispatch counters. An accepted task, or a workflow that has moved on, is refused. A second submission for an accepted task is refused, so results are never duplicated; downstream stages start only after validation.

---

## 9. Troubleshooting

| You see | Do |
|---|---|
| `eaeh: command not found` / config not found | Repeat 2.3 and open a new terminal |
| `no active project` | `cd` into the project folder |
| `NOT READY` | Run the fix printed under each `BLOCK` line (often 6.3) |
| Nothing happens after `workflow run` | `eaeh worker status`, then `eaeh worker log` |
| Task `blocked: context_delivery_failed` | The AI could not read its task package (3 tries, none counted as an attempt). `eaeh worker history` shows what it retrieved; check `eaeh worker log`, update Claude Code, then `eaeh worker retry <TASK-ID> --workflow <WORKFLOW-ID>` (section 8.1) |
| Task `blocked: context_access_denied`, "worker and MCP server are different builds", or tools "denied … needs approval" | The worker was started before an upgrade. `eaeh worker stop`, then `eaeh worker start`, then `eaeh worker retry <TASK-ID> --workflow <WORKFLOW-ID>` |
| `context_integrity_failed` | The AI mixed package versions or damaged a section; EAEH delivers again by itself (up to 3 times) |
| Task `no_submission` | The AI had its task (acknowledged) but ended without a result: a normal attempt, retried up to 3 times |
| Task `blocked: usage_limit` / `auth_unavailable` | Wait for the reset / `claude` → `/login`; then `eaeh worker retry <TASK-ID>` |
| `FAILED at stage …` | Fix the cause; start a new run: `eaeh workflow run design --yes` |
| `BLOCKED at '<gate>'` / documents missing in `docs/architecture/` | `eaeh artifact status` explains why; fix it, then `eaeh artifact sync` |
| `not written – existing files would be overwritten` | Move or commit your own file there, then `eaeh artifact sync` (or `--replace-modified` to replace it with the approved version) |
| Portal: `ERR_CONNECTION_REFUSED` | The portal isn't running. Start it (section 4) and keep its terminal open |
| Portal: no `launch=` secret | The link is printed once in the portal's terminal. If it's gone, Ctrl-C and start again with `--open` |
| Portal: a project is missing | The portal uses another installation. Compare the data dir at the top of the portal with `eaeh project list`; the DEMO (port 8190) is separate |
| `port 8090 is already in use` | Another portal is running: `lsof -nP -iTCP:8090 -sTCP:LISTEN`, stop it, or use `--bind 127.0.0.1:<port>` |
| `approval decisions require an explicit approver identity` | Add `--as approver` |
| `eaeh setup` says `IDENTITY MISMATCH` | A profile holds another person's token. Check `eaeh auth status`; `eaeh setup --repair` replaces it with the right person's token (credential file backed up first) |
| `administrator credential unavailable` | `eaeh auth login --profile admin --token-file <FILE>` if you have the token; otherwise `eaeh setup --reset` (3.4) |
| `… has roles [x] but must have exactly [y]` | Setup never changes roles. Use `eaeh setup --reset` on a clean installation, or different people (`eaeh project init --developer …`) |
| `not confirmed – nothing was changed` | Type `RESET EAEH` when asked, or pass `--confirm-reset "RESET EAEH"` |
| `Skill approval … NOT available to agents` | Expected: approve in the portal (**Skills**) or `eaeh approval review --as approver` |
| `External AI … not ready` | Install Claude Code and run `claude` → `/login` (subscription, not an API key) |
| `the web UI is not built` | `npm --prefix portal ci && npm --prefix portal run build` |
| `port conflict` | See section 4 |
| Portal `403 … principal 'dev' may not access the target of workflow.read` | `dev` lacks access to that project. Run `eaeh setup --stop-portal` then `eaeh setup`: it grants access to every project |
| `bind … must be a loopback` | The portal has no login, so only `127.0.0.1` is allowed |

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

**Environment variables:** `EAEH_CONFIG` (installation), `EAEH_HOME` (where profiles, backups and portal state are stored), `EAEH_PROFILE` (default `--as`).

**Every command** has `--help`. Global options: `--config <FILE>`, `--as <PROFILE>`, `--json`.

**Deeper documentation:**
* [Governance Portal](../implementation/EAEH-GOVERNANCE-PORTAL.md)
* [Approved documents in the workspace](../implementation/EAEH-ARTIFACT-MATERIALIZATION.md)
* [Architecture decisions](../architecture/adr/)
