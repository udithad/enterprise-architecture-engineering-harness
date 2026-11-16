# Legacy Integration Adapter Design

A Claude Code skill for designing anti-corruption adapters that translate between a new system's clean domain model and a legacy system's protocols, quirks, and data quality issues — so legacy debt stays contained at the boundary instead of leaking into new code.

## Who it helps

- **Architects and tech leads** designing how a new service integrates with a mainframe, old monolith, or third-party system whose data model shouldn't shape the new system's design.
- **Developers** building the translation layer who need a concrete pattern for handling overloaded fields, magic values, and inconsistent legacy data without silently corrupting the new domain model.
- **Engineering managers** running a `strangler-fig-migration-pattern` who need legacy and new systems to interoperate safely during a long transition.

## How it helps

- Establishes the anti-corruption layer (ACL) pattern: model the new domain cleanly first, then isolate every legacy quirk and protocol concern behind a single translation point.
- Gives a concrete workflow for defensive data handling — explicit translation tables, documented failure policy, and monitoring translation failures as a signal about undocumented legacy behavior.
- Walks through a full worked example: a bank's loan-origination service translating a 25-year-old mainframe's overloaded status codes, sentinel dates, and an undocumented bitfield interaction into a clean domain model.
- Covers the adapter's eventual retirement as part of a migration, so it doesn't outlive its purpose and become permanent unexplained infrastructure.

## How to use it

As a Claude Code skill, drop this folder into a project so Claude can invoke it automatically when a legacy integration needs designing:

```bash
mkdir -p .claude/skills
cp -r legacy-integration-adapter-design .claude/skills/
```

Or place it under `~/.claude/skills/` to make it available in every project on your machine. It can also be distributed to a team via a Claude Code plugin marketplace, where it's installed the same way any plugin skill is.

## Download just this skill

This skill lives inside the larger `software-architecture-skills` repo. Use a sparse checkout to fetch only this folder:

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/legacy-integration-adapter-design
```

Then copy it into your project:

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/legacy-integration-adapter-design /path/to/your/project/.claude/skills/
```
