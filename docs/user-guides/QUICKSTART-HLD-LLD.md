# Quick Start: an API project from BRD to HLD and LLD

This guide takes a small API project from a BRD to an approved high-level design (HLD) and low-level design (LLD).

- **Claude Code** does the writing. EAEH runs it for you automatically, on your Claude subscription.
- **EAEH** checks the results and asks a person to approve each stage.
- **You** start the work once, then only review and approve.

## What you need

- The EAEH repository, built once (step 1).
- Your API project folder, containing a BRD written in Markdown, for example `BRD.md`.
- Claude Code, signed in with your Claude subscription. Check with `claude auth status`: it should show `"authMethod": "claude.ai"`. (If not, run `claude` and use `/login`.)

You don't need an AI API key. EAEH never uses one, and refuses to run Claude Code if it is signed in with one.

## 1. Build EAEH (once)

In the EAEH repository:

```bash
scripts/ci.sh
export PATH="$PWD/target/debug:$PATH"
export EAEH_CONFIG="$PWD/config/eaeh.toml"
eaeh init
```

## 2. Create the administrator (once)

```bash
eaeh admin bootstrap --tenant acme --project platform --admin-id admin --token-file ./admin.token
eaeh auth login --profile admin --token-file ./admin.token && rm ./admin.token
```

## 3. Set up your project (one command)

Go to your API project folder and run:

```bash
cd ~/projects/my-api
eaeh project init --name my-api \
  --language "Java 21" --framework "Spring Boot 3.x" --build-tool Maven \
  --brd BRD.md \
  --developer dev --steward steward --approver approver \
  --store-profiles
```

This command:
- registers the project and stores your BRD;
- creates three people: **dev** runs the work, **approver** signs it off and **steward** looks after knowledge;
- writes the files an AI assistant needs if you also want to use it interactively: `CLAUDE.md`, `.mcp.json`, `.github/copilot-instructions.md` and `.vscode/mcp.json`.

It ends with **NOT READY**. That is expected: the next step fixes it.

## 4. Approve the skills

The AI uses predefined "skills", for example how to write an HLD. A person must approve them first. Read the list, then approve it:

```bash
eaeh approval review --as approver
eaeh approval review --as approver --all --yes
```

Then check:

```bash
eaeh project readiness
```

You should see **READY**, including a line like `ok [automation] automated AI execution ready: Claude Code … subscription login`. Warnings are fine. Any blocking item comes with the command that fixes it.

## 5. Start the design work

```bash
eaeh workflow run design --yes
```

EAEH starts the BRD → requirements → HLD → LLD process and a background worker. The worker runs Claude Code for each AI step: requirements analysis, then requirements refinement. Nothing else to do.

Want to follow along? (Optional; Ctrl-C stops watching, not the work.)

```bash
eaeh workflow watch
```

## 6. Review and approve each stage

When a document is ready, you get a desktop notification ("EAEH: approval needed"). Then run:

```bash
eaeh review --as approver
```

It shows the document (a copy is in `review/<stage>/`) and asks:

- **a**: approve. The next stage starts automatically.
- **c**: request changes. Type what to change; Claude Code revises the document automatically and you review it again.
- **r**: reject. The run stops.

Do this three times:

1. **Requirements**
2. **HLD** (high-level design)
3. **LLD** (low-level design)

When `eaeh workflow status` shows `succeeded`, you're done.

## 7. Find your results

The folders under `review/` contain:

| Stage | Files |
|---|---|
| Requirements | `requirements.md` |
| HLD | `hld.md`, ADRs in `adr/`, diagrams in `diagrams/` |
| LLD | `lld.md` and its diagrams |

To see how a requirement links through to the design:

```bash
eaeh trace --id REQ-F-001
```

## If something goes wrong

Start with `eaeh worker status`: it shows whether Claude Code is ready and why a step stopped.

| You see | Do this |
|---|---|
| `NOT READY` | Run the fix command shown next to each `BLOCK` line |
| `approval decisions require an explicit approver identity` | Add `--as approver` |
| A step `blocked … usage_limit` | Your subscription limit was reached. When it resets: `eaeh worker retry <task-id>` |
| A step `blocked … auth_unavailable` | Sign in to Claude Code (`claude`, then `/login`), then `eaeh worker retry <task-id>` |
| `billing_unsafe` | Claude Code is signed in with an API key. Sign in with your subscription instead |
| Nothing happens after a restart | `eaeh worker start` |
| `no credentials` | Repeat step 2 |

More detail: [User Manual](EAEH-USER-MANUAL.md) (everything, by role), [Developer Experience Guide](DEVELOPER-EXPERIENCE-GUIDE.md).
