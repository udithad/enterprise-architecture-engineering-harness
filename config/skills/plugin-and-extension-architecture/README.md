# Plugin and Extension Architecture

A Claude Code skill for designing extension points — a plugin API, hook system, or SDK — so third parties or other teams can extend a system's behavior without forking or modifying core code, and without every addition requiring a core redeploy.

## Who it helps

- **Architects and tech leads** deciding where true extension points exist in a system and how to design a stable contract around them before the first plugin locks in a bad shape.
- **Engineering managers** who need release cadence decoupled — plugins updatable independently of the core system — and third-party risk (untrusted code) explicitly managed rather than assumed away.
- **Developers** building a plugin/SDK system who need a concrete pattern for trust tiers, failure isolation, and independent API versioning.

## How it helps

- Distinguishes genuine extension points (where behavior really varies) from complexity that just looks like it needs a plugin system.
- Gives an explicit trust-model framework — first-party vs. third-party plugins get different access and isolation levels, not implicit uniform trust.
- Covers the parts of plugin architecture most designs skip until it's too late: failure isolation, independent versioning, per-plugin observability, and resource limits.
- Walks through a full worked example: a form-builder SaaS designing a sandboxed, versioned, per-handler-observable plugin system that let 40+ community integrations ship without core team involvement.

## How to use it

```bash
mkdir -p .claude/skills
cp -r plugin-and-extension-architecture .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/plugin-and-extension-architecture
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/plugin-and-extension-architecture /path/to/your/project/.claude/skills/
```
