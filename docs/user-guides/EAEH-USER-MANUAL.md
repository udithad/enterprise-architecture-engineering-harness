# EAEH User Manual

The complete instruction set for using the Enterprise Architecture Engineering Harness (EAEH), from installation to delivered code. It is organised by **who does what**.

- New to EAEH? Read [Getting Started](EAEH-GETTING-STARTED.md) first (10 minutes).
- Just want an HLD/LLD for a small API? Follow the [Quick Start](QUICKSTART-HLD-LLD.md).
- Every option and error message in detail: [Developer Experience Guide](DEVELOPER-EXPERIENCE-GUIDE.md).

---

## Where to run each command

You work in **two different folders**. Every command block below starts with a 📁 line that says which one.

| Name used in this manual | What it is | Example path used below |
|---|---|---|
| **EAEH folder** | The EAEH repository you cloned and built. EAEH itself lives here. **Never** set up a project in it. | `~/repo/enterprise-architecture-engineering-harness` |
| **Project folder** | The repository of the system you are designing/building (one per project). It holds the BRD; EAEH writes `.eaeh/project.toml`, `review/`, `CLAUDE.md` … here. | `~/projects/simple-springboot-api` |

- 📁 **EAEH folder**: run there (`cd ~/repo/enterprise-architecture-engineering-harness`).
- 📁 **Project folder**: run there (`cd ~/projects/simple-springboot-api`). EAEH finds the project from `.eaeh/project.toml` in this folder; elsewhere the command fails with `no active project`.
- 📁 **Any folder**: the command does not depend on the folder.

The example paths are examples: use the folders where *you* cloned EAEH and keep your project. A wrong EAEH path in `~/.zshrc` is the most common setup error (see the check at the end of [section 2](#2-install-once-per-machine)).

All commands work from any folder only because of the two `export` lines in [section 2](#2-install-once-per-machine) (`PATH` and `EAEH_CONFIG`). Put them in your shell profile (`~/.zshrc`) so every new terminal has them.

Replace the example paths with your own.

## How to read the commands

- Text in `<ANGLE-BRACKETS>` is a **placeholder**: replace it, brackets included, with your own value. Never type the brackets.
- Everything else is typed exactly as shown. Options with no `<…>` value are complete as written.
- Lines starting with `#`, and text after `#`, are comments; you don't need to type them.
- Where a section shows a **Template** and an **Example**, the example is the template with every placeholder filled in.

Every placeholder used in this manual, and where to find its value:

| Placeholder | Meaning | Where the value comes from |
|---|---|---|
| `<PERSON-ID>` | A person's login id, e.g. `maria` | You choose it in `eaeh principal create --id`; `eaeh project init` creates `dev`, `steward`, `approver` |
| `<NAME>` | Display name, e.g. `"Maria Silva"` | You choose it |
| `<ROLE>` | One role, or several separated by commas | `admin`, `operator`, `approver`, `viewer`, `auditor`, `knowledge_steward` |
| `<LABEL>` | A name for a token, e.g. `"maria laptop"` (max. 100 characters) | You choose it; shown by `eaeh token list --principal <PERSON-ID>` |
| `<FILE>` | A file path. A plain name (`notes.md`) means the folder you are in now; a full path (`~/notes.md`) works from any folder. **This manual uses full paths (`~/…`) for every file that one command writes and a later command reads.** | Your file; it must exist (except output files such as a new token file) |
| `<PROFILE>` | Name under which a token is stored on your machine, e.g. `maria` | You choose it in `eaeh auth login --profile`; list them with `eaeh auth status` |
| `<PROJECT-ID>` | The project's long id, e.g. `01a1d557-883e-7450-8f1a-d38db85c3b2a` (not its name) | `eaeh project list`, or `id = …` under `[project]` in `.eaeh/project.toml` in the project folder |
| `<TOKEN-ID>` | A token's id | `eaeh token list --principal <PERSON-ID>` (field `id`) |
| `<SOURCE>` | A knowledge source name, e.g. `team-guides` | You choose it in `eaeh knowledge source-create --name`; list with `eaeh knowledge sources` |
| `<ITEM>` | A pending skill or standard | `eaeh approval review --as approver` lists them (use the skill name such as `hld-design@1.0.0`, or the approval id) |
| `<APPROVAL-ID>` | A document waiting for approval | `eaeh review --as approver` (when several wait) or `eaeh workflow status` |
| `<RUN-ID>` | A workflow run | `eaeh workflow list` or `eaeh workflow status` (first line) |
| `<TASK-ID>` | One AI task of a run | `eaeh worker status` ("AI tasks waiting") or `eaeh task list --all` |
| `<ARTIFACT-ID>` | A stored document or bundle | `eaeh workflow status` (artifacts with ids) |
| `<BUNDLE-ID>` | The delivered source bundle (an artifact) | `eaeh workflow status`: the artifact of stage `snapshot` |
| `<REASON>` | Your comment for a rejection or change request | You write it |
| `<FOLDER>` | A folder path | Your folder |

---

## Contents

1. [How EAEH works](#1-how-eaeh-works)
2. [Install (once per machine)](#2-install-once-per-machine)
3. [Administrator](#3-administrator)
4. [Knowledge steward](#4-knowledge-steward)
5. [Approver](#5-approver)
6. [Developer: everyday work](#6-developer-everyday-work)
7. [Automated AI execution (the worker)](#7-automated-ai-execution-the-worker)
8. [Using an AI assistant interactively](#8-using-an-ai-assistant-interactively)
9. [Implementation and tests (deliver)](#9-implementation-and-tests-deliver)
10. [Results, traceability and audit](#10-results-traceability-and-audit)
11. [Execution modes and costs](#11-execution-modes-and-costs)
12. [Configuration reference](#12-configuration-reference)
13. [Command reference](#13-command-reference)
14. [Troubleshooting](#14-troubleshooting)
15. [Limits](#15-limits)

---

## 1. How EAEH works

EAEH lets AI do engineering work under enterprise control:

| | Who | Does |
|---|---|---|
| **Proposes** | Claude Code (your subscription), run by EAEH | Writes requirements, HLD, LLD, implementation plan, code and tests |
| **Checks** | EAEH | Schemas, citations, BRD coverage, standards, quality gates, permissions |
| **Decides** | People | Approve, request changes, or reject at each gate |
| **Records** | EAEH | Versioned artifacts, traceability links, tamper-evident audit log |

**The lifecycle:**

```
BRD ─► requirements analysis ─► requirements refinement ─► requirements.md ─► [approve]
    ─► HLD ─► [approve] ─► LLD ─► [approve]                      (activity: design)
    ─► implementation plan ─► [approve] ─► code + tests ─► (build) ─► [approve]   (activity: deliver)
```

You start an activity once. EAEH runs every AI step automatically and stops only at the `[approve]` gates.

**Roles** (one person may hold several, but never approve their own work):

| Role | Profile name | Does | Cannot |
|---|---|---|---|
| Administrator | `admin` | Install, bootstrap projects, create people, issue tokens, read audit | Run workflows, approve |
| Developer (operator) | `developer` | Run workflows, run the worker, read results | Approve, ingest standards, register skills |
| Knowledge steward | `steward` | Ingest BRDs, knowledge and standards; register skills | Approve |
| Approver | `approver` | Approve / request changes / reject documents, skills and standards | Run AI tasks, approve work they took part in |

---

## 2. Install (once per machine)

**You need:**
- Rust toolchain (pinned by `rust-toolchain.toml`) and `python3`.
- Claude Code (tested: 2.1.323), signed in **with your Claude subscription**:
  ```bash
  claude            # then type /login and sign in with your Claude account
  claude auth status   # must show "authMethod": "claude.ai"
  ```
  Do **not** sign Claude Code in with an API key: EAEH refuses to use it that way.
- Optional, for build verification: `podman` or `docker`.

**Build and initialise** — 📁 **EAEH folder**:

```bash
cd ~/repo/enterprise-architecture-engineering-harness
scripts/ci.sh                               # builds and tests; expect "==> CI PASSED"
eaeh_dir="$PWD"
export PATH="$eaeh_dir/target/debug:$PATH"
export EAEH_CONFIG="$eaeh_dir/config/eaeh.toml"
eaeh init                                   # data directory, database, definitions
eaeh health
```

Make it permanent (so `eaeh` works in every terminal and every folder). Run this **in the same terminal, still in the EAEH folder**: it writes the folder's *real* path (`$eaeh_dir`) into `~/.zshrc`, so it is correct wherever you cloned EAEH. Remove any older EAEH `export` lines from `~/.zshrc` first.

```bash
echo "export PATH=\"$eaeh_dir/target/debug:\$PATH\"" >> ~/.zshrc
echo "export EAEH_CONFIG=\"$eaeh_dir/config/eaeh.toml\"" >> ~/.zshrc
```

Check in a **new** terminal, from another folder:

```bash
cd ~
which -a eaeh      # the FIRST line must be <EAEH folder>/target/debug/eaeh
echo $EAEH_CONFIG  # must be an existing file: <EAEH folder>/config/eaeh.toml
eaeh project --help   # must list "init" and "readiness"
eaeh health
```

If `which -a eaeh` shows another `eaeh` first (for example an old `~/.cargo/bin/eaeh` from `cargo install`), remove it with `cargo uninstall eaeh-cli` (or `rm ~/.cargo/bin/eaeh`). An old binary is missing commands: `eaeh project --help` then lists only `create`, `list` and `set-domain`.

The shipped `config/eaeh.toml` is ready to use (`external_ai` mode with automated execution). See [section 12](#12-configuration-reference) to change it.

---

## 3. Administrator

### 3.1 Bootstrap the installation (once)

📁 **Any folder** (the token file is written to your home folder, `~/admin.token`, stored, then deleted):

```bash
eaeh admin bootstrap --tenant acme --project platform --admin-id admin --token-file ~/admin.token
eaeh auth login --profile admin --token-file ~/admin.token && rm ~/admin.token
```

### 3.2 Bootstrap a project (one command)

📁 **Project folder** — never the EAEH folder (EAEH refuses that). The BRD and any other file you name must be in this folder. Example: create the project folder and copy in the sample BRD that ships with EAEH:

```bash
mkdir -p ~/projects/simple-springboot-api
cd ~/projects/simple-springboot-api
git init
cp "$(dirname "$EAEH_CONFIG")/../examples/simple-springboot-api/BRD-Simple-Spring-Boot-API.md" .   # from the EAEH folder

eaeh project init --name simple-springboot-api \
  --description "Simple REST API" \
  --language "Java 21" --framework "Spring Boot 3.x" --build-tool Maven \
  --constraint "No database" --constraint "No authentication" \
  --brd BRD-Simple-Spring-Boot-API.md \
  --developer dev --steward steward --approver approver \
  --build-verification deferred \
  --store-profiles
```

Optional extras, only if you have such files: `--knowledge <FILE>` (e.g. architecture notes) and `--standard <FILE>` (a mandatory standard, e.g. your API error format), each repeatable.

| Option | Meaning |
|---|---|
| `--name`, `--description` | Project identity. `--name` is required |
| `--language`, `--framework`, `--build-tool`, `--constraint` | Technology stack and constraints given to the AI |
| `--brd` | The business requirements document (stored confidential, project-only). Required |
| `--knowledge` / `--standard` | Optional: extra knowledge / mandatory standards (standards need approval) |
| `--developer`, `--steward`, `--approver` | People to create (or give access to) with exactly those roles |
| `--store-profiles` | **Single-person setup:** store all three tokens as local profiles |
| `--handover-dir <FOLDER>` | **Team setup:** write each person's token to a private file for hand-over |
| `--build-verification` | `required` (EAEH builds and tests in a container) or `deferred` (marked unverified) |
| `--build-program`, `--build-arg` | Build command for `required` |
| `--workflow` | Workflows to enable (default: requirements, design, delivery) |
| `--client` | AI assistant files: `claude-code`, `github-copilot` |

It creates, **in the project folder**, `.eaeh/project.toml` (safe to commit), `CLAUDE.md`, `.mcp.json`, `.github/copilot-instructions.md` and `.vscode/mcp.json` (no secrets). From now on, every project command runs in this folder. It ends with a readiness report; **NOT READY** (exit 7) is expected until the skills are approved ([section 5.1](#51-approve-skills-and-standards)). Running it again is safe.

### 3.3 People and tokens

Only needed for a **team**. A single person who used `--store-profiles` in 3.2 already has the `developer`, `steward` and `approver` profiles and can skip this section.

**Add a person and give them a token** — 📁 **any folder**, as the administrator:

Template:

```bash
eaeh principal create --id <PERSON-ID> --display-name "<NAME>" --roles <ROLE> --projects all
eaeh token issue --principal <PERSON-ID> --label <LABEL> --token-file ~/<PERSON-ID>.token
```

Example:

```bash
eaeh principal create --id maria --display-name "Maria Silva" --roles approver --projects all
eaeh token issue --principal maria --label "maria laptop" --token-file ~/maria.token
```

- `token issue` needs all three options: `--principal` (who), `--label` (a name for this token, so you can recognise it in `eaeh token list --principal <PERSON-ID>`; it does not change permissions) and a destination. The destination is `--token-file <FILE>` (recommended; a private file you hand to the person), or `--store-profile <PROFILE>` (into your own credential store), or `--print` (shows it on screen; avoid).
- `--ttl-days <N>` sets the expiry (default 30 days).
- The token file is written exactly where `--token-file` says. A plain name like `maria.token` means "in the folder I am in now", so use a full path such as `~/maria.token` (your home folder) to find it easily later.
- Hand `~/maria.token` to Maria securely (if she uses another machine, she puts it in her home folder), then delete your copy.

**Give the person access to the project** — 📁 **project folder** (it uses the project of the folder you are in):

```bash
cd ~/projects/simple-springboot-api
eaeh principal grant --id maria
```

**The person stores their token once** — 📁 **any folder**, on their own machine, as themselves:

Template:

```bash
eaeh auth login --profile <PROFILE> --token-file ~/<PERSON-ID>.token && rm ~/<PERSON-ID>.token
```

Example:

```bash
eaeh auth login --profile maria --token-file ~/maria.token && rm ~/maria.token
eaeh auth status          # shows the stored profiles; never the tokens
```

If the token file was written somewhere else (for example, `token issue` was run with a plain name like `maria.token` in another folder), find it and use its full path:

```bash
find ~ -name "maria.token" 2>/dev/null          # e.g. /Users/you/repo/test-harness/maria.token
eaeh auth login --profile maria --token-file /Users/you/repo/test-harness/maria.token && rm /Users/you/repo/test-harness/maria.token
```

If `find` prints nothing, the file no longer exists. Usually an earlier `auth login … && rm …` already stored the token and deleted the file. Check with `eaeh auth status`:
- `maria … valid`: done; nothing to do.
- no `maria` line, or not `valid`: the administrator issues a new token (with a full path, e.g. `--token-file ~/maria.token`) and Maria logs in with it.

From then on Maria acts with `--as maria` (for approvals: `eaeh review --as maria`).

**Manage tokens and people** — 📁 **any folder**, as the administrator:

```bash
eaeh token list                          # your own tokens: id, label, expiry
eaeh token list --principal <PERSON-ID>  # someone else's tokens, e.g. --principal maria
eaeh token revoke <TOKEN-ID>             # <TOKEN-ID> = the "id" shown by token list
eaeh principal disable <PERSON-ID>       # e.g. eaeh principal disable maria
```

---

## 4. Knowledge steward

Optional. `eaeh project init` (3.2) already stored the BRD. Use this section to replace the BRD, or to add standards and other knowledge. Run as the steward (`--as steward`; with a single stored steward profile it is chosen automatically).

📁 **Project folder** for all commands (the files are read from there). The files named below are **your own**; they must exist.

**Replace the BRD with a revised version:**

Template / example:

```bash
cd ~/projects/simple-springboot-api
eaeh brd ingest --file <FILE>
eaeh brd ingest --file BRD-Simple-Spring-Boot-API-v2.md
```

**Add a mandatory standard** (designs are checked against it once an approver approves it):

Template:

```bash
eaeh knowledge source-create --name <SOURCE> --kind standard --description "<NAME>"     # once per source
eaeh knowledge ingest --file <FILE> --source <SOURCE> --scope enterprise --mandatory --project <PROJECT-ID>
```

Example (with a file `api-error-format.md` you wrote, in the project folder):

```bash
eaeh knowledge source-create --name enterprise-standards --kind standard --description "Enterprise standards"
eaeh knowledge ingest --file api-error-format.md --source enterprise-standards --scope enterprise --mandatory --project 01a1d557-883e-7450-8f1a-d38db85c3b2a
```

Then the approver approves it (5.1).

**Add other project knowledge** (one file, or every `.md`/`.txt`/`.json` file of a folder):

Template:

```bash
eaeh knowledge source-create --name <SOURCE> --kind reference --description "<NAME>"   # once per source
eaeh knowledge ingest --file <FILE> --source <SOURCE> --scope project:<PROJECT-ID> --project <PROJECT-ID>
eaeh knowledge import <FOLDER> --source <SOURCE> --scope project:<PROJECT-ID> --project <PROJECT-ID>
```

**Look around:**

```bash
eaeh knowledge sources
eaeh knowledge search --project <PROJECT-ID> --query "error format"
eaeh skill pending          # skills the project's workflows need, and their approval state
eaeh skill sync             # register required skills that are missing (then 5.1)
```

Notes:
- `--project` names the project whose approver approves the document; `--scope` says who may read it: `enterprise`, `domain:<name>`, `project:<PROJECT-ID>`, `team:<name>`, `workflow:<id>`. Without `--scope`, `ingest` and `import` use `enterprise`.
- Source kinds: `standard`, `reference`, `api_spec`, `adr`, `process`.
- A BRD classified `restricted` is never handed to an AI client.

---

## 5. Approver

Approvals always need your explicit identity: add `--as approver` (it is never chosen automatically).

### 5.1 Approve skills and standards

📁 **Project folder** (it lists what this project needs):

```bash
cd ~/projects/simple-springboot-api
eaeh approval review --as approver                 # 1. read every pending item in full (nothing is decided)
eaeh approval review --as approver --all --yes     # 2. approve all listed items
```

To decide items one by one, name them (`<ITEM>` = the skill name or approval id from step 1); without `--yes`, EAEH asks `y/N` for each:

```bash
eaeh approval review --as approver <ITEM>
eaeh approval review --as approver hld-design@1.0.0
eaeh approval review --as approver <ITEM> --reject --comments "<REASON>"
```

Then check: `eaeh project readiness` should say **READY**.

### 5.2 Review documents at workflow gates

You get a desktop notification ("EAEH: approval needed"). Then, 📁 **project folder**:

```bash
cd ~/projects/simple-springboot-api
eaeh review --as approver
```

1. A copy of the document is written to `review/<gate>/` **in the project folder** (e.g. `~/projects/simple-springboot-api/review/approve_hld/hld.md`) and checked against the version under approval.
2. The main document is printed (`--open` opens it in your viewer; `--no-show` skips printing).
3. You choose:
   - **a**: approve. The next stage starts automatically.
   - **c**: request changes, with comments. The AI revises it automatically; you review the new version (up to 3 rounds per gate). The old version and your comments are kept.
   - **r**: reject, with comments. The run ends.

Without a terminal prompt (scripts) — 📁 **project folder**. `<APPROVAL-ID>` is shown by `eaeh review --as approver` and `eaeh workflow status`:

```bash
eaeh review <APPROVAL-ID> --as approver --decision approve --comments "Looks good"
eaeh review <APPROVAL-ID> --as approver --decision request_changes --comments "Add a caching decision"
eaeh review <APPROVAL-ID> --as approver --decision reject --comments "<REASON>"
```

Each decision records your identity, role, time, comments and the artifact's hash. You cannot approve anything you authored or submitted, and nothing else can approve for you: the worker and AI assistants have no approval capability.

---

## 6. Developer: everyday work

📁 **Project folder** for everything in this section:

```bash
cd ~/projects/simple-springboot-api
eaeh project readiness                 # READY / NOT READY, with the fix for every problem
eaeh workflow run design               # plan: stages, agents, skills, gates (nothing starts)
eaeh workflow run design --yes         # start; the worker runs the AI steps automatically
eaeh workflow watch                    # live progress (Ctrl-C stops watching, not the work)
eaeh workflow status                   # current stage, history, artifacts, next actions
```

Activities:

| Activity | Runs | Gates |
|---|---|---|
| `requirements` | BRD → approved requirements | requirements |
| `design` | BRD → requirements → HLD → LLD | requirements, HLD, LLD |
| `deliver` (aliases `plan`, `implement`, `test`) | implementation plan → code and tests → bundle | plan, delivery |

Other run commands — 📁 **project folder**:

```bash
eaeh workflow run design --yes --follow      # start and watch in one command
eaeh workflow run design --yes --no-worker   # start without automated execution
eaeh workflow list                           # all runs, with their <RUN-ID>
eaeh workflow resume <RUN-ID>                # continue an interrupted run
eaeh workflow cancel <RUN-ID>
```

A failed or rejected run is final (everything is kept); start a new run after fixing the cause.

---

## 7. Automated AI execution (the worker)

**How it works.** When a run waits for AI work, EAEH starts a background worker for the project (you don't start it yourself). Claude Code runs in a private empty folder that EAEH creates; it never works in your project folder or the EAEH folder. For each AI task it:
1. checks Claude Code is installed and signed in with a **subscription** (refuses API-key sign-in);
2. starts `claude -p` in an empty private folder with **only four EAEH tools**, for that one task (read the task, call the agent's allowed tools, check a draft, submit); no shell, file or web access;
3. Claude Code builds the result from the task's exact format (`output_outline` and a valid `output_example`, generated from the schema EAEH enforces), checks drafts for free with `eaeh_task_validate` (every problem listed, no correction attempt used), then submits; EAEH validates it again and Claude Code fixes any rejection in the same session;
4. moves on to the next task, or stops at a gate and notifies you;
5. records the attempt: provider, Claude Code version, sign-in status, timing, outcome, retries.

**Control it** — 📁 **project folder** (each project has its own worker; the commands act on the project of the folder you are in), except `worker retry <TASK-ID>`, which works in any folder:

```bash
cd ~/projects/simple-springboot-api
eaeh worker status              # mode, Claude Code version and sign-in, worker, waiting tasks
eaeh worker history             # every attempt and its outcome
eaeh worker log                 # the worker's log
eaeh worker stop                # stop (the running task is retried later)
eaeh worker start               # start (e.g. after a reboot)
eaeh worker retry <TASK-ID>     # continue a stopped task after fixing the cause (<TASK-ID> from `eaeh worker status`)
eaeh worker run --until-idle    # run in the foreground until a person is needed (scripts, CI)
```

**When a task stops:**

| State | Cause | What to do |
|---|---|---|
| retrying | Timeout, invalid result, crash (attempts 1–2) | Nothing; it retries after a growing delay |
| `exhausted` | All attempts (default 3) failed | Check `eaeh worker history` / `eaeh task show <TASK-ID>`; then `eaeh worker retry <TASK-ID>` |
| `blocked: usage_limit` | Subscription usage limit reached | Wait for the reset; `eaeh worker retry <TASK-ID>` |
| `blocked: auth_unavailable` | Claude Code not signed in / expired | `claude` → `/login`; then retry |
| `blocked: billing_unsafe` | Claude Code signed in with an API key / cloud provider | Sign in with your subscription; then retry |
| `blocked: cli_unavailable` | Claude Code missing or too old | Install / update; then retry |

A stopped task never fails the run; it waits for you. If the worker or machine stops mid-task, the next worker picks the task up again.

**Billing safety:** EAEH makes no model API calls itself, never passes API keys to Claude Code, stops any session that is not on the subscription login, and never falls back to another provider.

---

## 8. Using an AI assistant interactively

Optional. Open the **project folder** (not the EAEH folder) in Claude Code (`cd ~/projects/simple-springboot-api && claude`) or in VS Code with GitHub Copilot (agent mode), and allow the `eaeh` MCP server when asked. The `eaeh` server is defined by `.mcp.json` / `.vscode/mcp.json` in the project folder; in any other folder it is not available. You can then ask, for example:

- *"What's the EAEH status of this project?"*
- *"Show me the pending approvals."*
- *"Explain how EAEH would run the design activity."*

The assistant has EAEH tools for context, readiness, knowledge search, activities, status, approvals (read-only), artifacts and traces. It has **no approval tool**. If the worker is not used (`--no-worker`), ask it: *"Do the EAEH task that is waiting"* and it uses `eaeh_task_get` / `eaeh_task_tool` / `eaeh_task_submit`. The same exists on the command line (`eaeh task list|show|tool|validate|submit`); `eaeh task validate <TASK-ID> --output <FILE>` checks a draft without submitting it.

---

## 9. Implementation and tests (deliver)

After a design run has succeeded — 📁 **project folder**:

```bash
cd ~/projects/simple-springboot-api
eaeh workflow run deliver          # plan
eaeh workflow run deliver --yes    # start
```

1. The AI writes an implementation plan (every Must requirement mapped to a task) → **you approve the plan**.
2. The AI writes code and tests into an isolated EAEH workspace (not your repository).
3. EAEH checks every Must requirement is implemented and tested, and bundles the files with trace links.
4. With `--build-verification required`: EAEH runs your build/test command in a container without network; a failure stops the run.
5. **You approve the bundle** (`eaeh review --as approver`).
6. Copy the approved code into the project folder — 📁 **project folder** (`--out .` means "here"):
   ```bash
   cd ~/projects/simple-springboot-api
   eaeh workflow status                              # <BUNDLE-ID> = the artifact of stage "snapshot"
   eaeh artifact export <BUNDLE-ID> --out .          # reports whether this version was approved
   ```

Build verification setup: an administrator sets, in `config/eaeh.toml` in the 📁 **EAEH folder**, `[isolation] container_image` (with your toolchain and dependencies) and allows the program in `exec_allowed_programs`; the project records it by re-running, in the 📁 **project folder**, `eaeh project init … --build-verification required --build-program /usr/bin/mvn --build-arg -B --build-arg verify`.

---

## 10. Results, traceability and audit

📁 **Project folder**, except the two `audit` commands (📁 **any folder**):

```bash
cd ~/projects/simple-springboot-api
eaeh review --as approver                       # the document waiting for you (copy in review/<gate>/)
eaeh artifact export <ARTIFACT-ID> --out out/   # any artifact or bundle (ids in `eaeh workflow status`), hash-checked
eaeh trace --id REQ-F-001 --depth 3             # BRD section → requirement → HLD → LLD → code → test
eaeh task list --all                            # every AI task, its submissions and client
eaeh worker history                             # every automated attempt
eaeh --as admin audit list --limit 500          # who did what, when
eaeh audit verify                               # the audit chain is intact
```

Outputs of `design`: `requirements.md` (+ JSON, BRD coverage), `hld.md` with ADRs and Mermaid diagrams, `lld.md` with data model, sequences and deployment.

---

## 11. Execution modes and costs

| Mode | AI by | Cost | Use |
|---|---|---|---|
| `external_ai` (default) | Claude Code via the worker (or your assistant) | Your Claude subscription and its usage limits; no API key | Normal use |
| `direct_provider` | EAEH calls the Anthropic API | Separate API key and bill (`EAEH_SECRET_ANTHROPIC_API_KEY`) | Servers without a subscription client; only if chosen explicitly |
| `deterministic` | Nobody: recorded answers replayed (**not AI**) | None | Tests and demos |

Hosting, storage and machines are separate costs in every mode. No subscription is promised to cover unlimited use. Claude Code's reported cost per attempt is recorded as *notional*; on a subscription login it is not a separate charge.

---

## 12. Configuration reference

The file is `config/eaeh.toml` in the 📁 **EAEH folder** (the path in `EAEH_CONFIG`). Edit it there; it applies to all projects. Most-used settings:

```toml
[storage]
data_dir = "../.eaeh-data"        # relative to config/, i.e. <EAEH folder>/.eaeh-data: database, artifacts, workspaces, worker logs

[execution]
mode = "external_ai"              # external_ai | direct_provider | deterministic

[execution.coordinator]           # automated execution
auto_start = true                 # start the worker when AI work waits
provider = "claude_code"          # only automated provider
# claude_path = "/usr/local/bin/claude"
# model = "sonnet"                # default: your Claude Code setting
task_timeout_secs = 1800          # per attempt
max_attempts = 3                  # 1–10
backoff_secs = 30                 # first retry delay, doubled per attempt
lease_secs = 120                  # silent worker presumed dead after this
notify = true                     # desktop notifications

[isolation]
container_enabled = true          # build verification needs a container runtime
# container_image = "…"
# exec_allowed_programs = ["/usr/bin/mvn"]

[api]
bind = "127.0.0.1:8080"           # HTTP API (eaeh serve)
```

Environment variables:

| Variable | Purpose |
|---|---|
| `EAEH_CONFIG` | Configuration file |
| `EAEH_HOME` | Where your profiles (tokens) are stored |
| `EAEH_PROFILE` | Profile to act as (same as `--as`) |
| `EAEH_TOKEN` | A token for one command (not used by the worker) |
| `EAEH_SECRET_ANTHROPIC_API_KEY` | Only for `direct_provider` |

---

## 13. Command reference

Global options: `--config <FILE>`, `--as <PROFILE>`, `--json`.

| Area | Commands | 📁 Where |
|---|---|---|
| Build EAEH | `scripts/ci.sh` | EAEH folder |
| Setup | `init`, `health`, `admin bootstrap`, `serve` (HTTP API) | Any folder |
| Identity | `auth login / status / use / renew / logout`; `token issue / list / revoke / rotate`; `principal create / disable / set-teams` | Any folder |
| Project access | `principal grant --id <PERSON-ID>` | Project folder (or `--project <PROJECT-ID>`) |
| Project | `project init / readiness` | Project folder |
| | `project list / create / set-domain` | Any folder |
| Knowledge | `brd ingest`; `knowledge ingest / import / list / search / conflicts / retire / delete` | Project folder |
| | `knowledge source-create / sources` | Any folder |
| Skills | `skill pending / sync` | Project folder |
| | `skill list / set-status`; `skill register --project <PROJECT-ID>` | Any folder |
| Approvals | `review`; `approval review / list` | Project folder |
| | `approval decide <APPROVAL-ID>` | Any folder |
| Workflows | `workflow run / watch / status / list` | Project folder |
| | `workflow show / resume / cancel <RUN-ID>`, `workflow start --project <PROJECT-ID> …`, `workflow definitions` | Any folder |
| Automated AI | `worker start / stop / status / history / log / run` | Project folder |
| | `worker retry <TASK-ID>` | Any folder |
| AI tasks (manual) | `task list`; `mcp` (started by your assistant) | Project folder |
| | `task show / tool / submit <TASK-ID>` | Any folder |
| Results | `artifact export`, `trace` | Project folder (or `--project`) |
| | `audit list / verify` | Any folder |
| Platform | `agents`, `tools`, `plugin list / enable / disable / retire` | Any folder |

Every command has `--help`. Errors print `{"error": {"code", "message", "hint"}}`; the `hint` is the fix. Exit codes: 3 credentials, 4 permission, 5 not found, 6 integrity, 7 not ready, 1 other.

---

## 14. Troubleshooting

| You see | Do this |
|---|---|
| `NOT READY` | Run the fix shown under each `BLOCK` line |
| `no credentials` | `eaeh auth login --profile <PROFILE> --token-file <FILE>` |
| `approval decisions require an explicit approver identity` | Add `--as approver` |
| `… lacks permission …` | Use the right profile, e.g. `--as developer` to run work |
| `the token of profile 'x' … expired` | Ask the administrator for a new token; renew earlier with `eaeh auth renew` |
| `… is the EAEH repository itself` | You are in the EAEH folder. `cd` to the project folder and run it again |
| `no active project: run inside a repository bootstrapped with eaeh project init …` | You are not in the project folder. `cd` to it (or add `--project <PROJECT-ID>`) |
| `eaeh auth status` shows a profile as `INVALID (revoked/disabled)` | Its token no longer works (e.g. the EAEH data was reset, or the token was revoked). For `developer` / `steward` / `approver`: run `eaeh project init` again in the project folder with the same options and `--store-profiles`; it replaces profiles that no longer work. For anyone else: the administrator issues a new token (3.3) |
| `cannot read token file …` but `eaeh auth status` shows the profile as `valid` | Nothing to do: an earlier `auth login … && rm …` already stored the token and deleted the file |
| `cannot read token file …` / `No such file or directory` | The file is not in the folder you are in. Use its full path (e.g. `--token-file ~/maria.token`); find it with `find ~ -name maria.token` |
| `error: the following required arguments were not provided` | An option is missing; the message names it. Compare with the template in this manual, or run the command with `--help` |
| `eaeh: command not found` / `configuration file … not found` | The two `export` lines from [section 2](#2-install-once-per-machine) are missing in this terminal |
| `file(s) not found: …` | A `--brd` / `--knowledge` / `--standard` path is wrong; nothing was changed. Fix it or drop the optional options |
| Nothing happens after starting a run | `eaeh worker status`, then `eaeh worker log` |
| A task `blocked` / `exhausted` | See the table in [section 7](#7-automated-ai-execution-the-worker) |
| `a worker is already running for this project` | Only one per project; `eaeh worker status` |
| After a reboot the run waits | `eaeh worker start` |
| `FAILED at stage …` in `workflow status` | Read the reason and the hint; fix it; start a new run |
| `BLOCK [delivery] no approved design` | Finish and approve a `design` run first |

More messages: [Developer Experience Guide, Troubleshooting](DEVELOPER-EXPERIENCE-GUIDE.md#15-troubleshooting).

---

## 15. Limits

- **One AI task at a time** per project; no parallel agents.
- **Automation needs Claude Code on this machine**, signed in, with the machine on. GitHub Copilot works interactively only (VS Code agent mode); its cloud agent cannot reach a local EAEH.
- **Subscription limits apply**; long runs may pause at `usage_limit`. Unattended use is subject to Claude Code's terms.
- **Confidential BRDs are sent to your AI client's provider** under your subscription's terms; mark a BRD `restricted` to prevent that.
- **Instructions are not enforcement** for an assistant you use interactively: it could edit files outside EAEH if asked. The automated worker's sessions have no file tools.
- **Not available yet:** release and deployment, a web interface, HTTP API equivalents of the newest commands (`task`, `project`, `auth`, `worker`, `review`).
- **Verification status:** the automated path is covered by automated tests (with a stand-in AI client) and real `claude -p` probe sessions; a full automated run with the real Claude Code is in progress.
