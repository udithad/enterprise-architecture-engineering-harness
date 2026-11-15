# Enterprise Architecture Engineering Harness (EAEH)

A Rust-first, governed engineering harness: deterministic workflow, policy, authorization,
approval and budget controls are authoritative; dynamically instantiated agents propose and
act only through governed, audited, isolated gateways.

**Status: Phase 3 – Requirements Engineering and Architecture Generation, implementation
complete, awaiting phase exit decision.** See
[`docs/implementation/stages/PHASE-03-COMPLETION.md`](docs/implementation/stages/PHASE-03-COMPLETION.md)
(Phases 1–2 accepted; reports in `docs/implementation/stages/`).

BRD analysis, HLD/LLD generation and a governed software-delivery workflow (implementation plan → code and tests in an isolated workspace → build/test → approval) are implemented. Each step has deterministic validation, human approval gates and traceability.

**Who does the AI work (ADR-0025).** By default (`[execution] mode = "external_ai"`), your **Claude Code or GitHub Copilot** subscription does the AI reasoning. EAEH hands each agent task to the assistant over MCP and validates the result. **EAEH makes no model calls and needs no AI API key of its own.**

Two other modes exist:
- `direct_provider`: EAEH calls its own Anthropic adapter. Opt-in, separately billed, and verified only against a mock server.
- `deterministic`: replayed fixtures; not AI.

**Getting started:** the [EAEH User Guide](docs/user-guides/EAEH-USER-GUIDE.md) is the single guide: clone, build, setup, Governance Portal, a no-AI demo, and a new project from BRD to approved Requirements, HLD and LLD, with the CLI command and the portal action for each step.

## Quick start

```bash
# prerequisites: rustup (toolchain 1.99.1 is selected automatically), Podman or Docker
podman pull docker.io/library/alpine:3.20     # for isolated exec.run (optional)

scripts/ci.sh                                 # fmt, clippy, build, all tests, cargo-deny
B=target/debug/eaeh
$B init
$B admin bootstrap --tenant acme --project platform --admin-id admin --token-file ./admin.token
$B auth login --profile admin --token-file ./admin.token && rm ./admin.token
$B serve                                      # http://127.0.0.1:8080/v1/health
scripts/smoke-api.sh                          # scripted end-to-end check of the real server
scripts/demo-knowledge.sh                     # knowledge + skills demo (no live model unless authorised)
scripts/demo-design.sh                        # BRD → requirements → HLD → LLD demo (live with a key file)
scripts/demo-onboarding.sh                    # simple-springboot-api onboarding, deterministic mode (no AI)
scripts/demo-external-ai.sh setup             # simple-springboot-api in external_ai mode (your AI client does the work)
```

## Repository layout

| Path | Contents |
|---|---|
| `crates/` | 22 crates (core, store, events, governance, extensibility, isolation, workspace, validation, tools, intelligence, knowledge, skills, provider-anthropic, requirements, architecture, engineering, agents, workflow, runtime, api, cli, testkit) |
| `tests/` | Workspace acceptance suites: architecture fitness, end-to-end, knowledge and design demos |
| `config/` | `eaeh.toml` (incl. `[execution]`, `[knowledge]`, `[providers.*]`), agent definitions, skills, organization policy directory |
| `prompts/` | Versioned prompt templates |
| `workflows/` | Workflow definitions (`demo.approval`, `engineering.task`, `knowledge.review`, `requirements.analysis`, `engineering.design`, `engineering.delivery`, `engineering.delivery-unverified`) |
| `examples/` | Labelled demonstration knowledge corpus, BRD, design fixtures and skills (not real enterprise content) |
| `scripts/` | `ci.sh`, `smoke-api.sh`, `demo-knowledge.sh`, `demo-design.sh`, `demo-onboarding.sh`, `demo-external-ai.sh` |
| `docs/architecture/` | Architecture, ADRs, threat model |
| `docs/implementation/` | Roadmap, action register, test strategy, phase reports, evidence |
| `docs/OPERATIONS.md` | Install, configure, run, CLI and API reference |
| `docs/user-guides/` | The [EAEH User Guide](docs/user-guides/EAEH-USER-GUIDE.md) |
| `claude-instructions.md` | Master architecture and implementation instructions |

## Licence

Apache-2.0 (see `LICENSE`).
