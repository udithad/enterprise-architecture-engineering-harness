# Domain-Driven Design Context Mapping

A Claude Code skill for mapping bounded contexts and the relationships between them — shared kernel, customer/supplier, anti-corruption layer, conformist — so teams stop assuming the same term means the same thing everywhere.

## Who it helps

- **Architects and tech leads** who need the domain model, not the database schema or org chart, to drive where service boundaries and integration layers go.
- **Engineering managers** mediating between teams whose integrations keep breaking because each side's model of a shared-sounding term (Customer, Order, Policy) has quietly diverged.
- **Developers** integrating with a legacy or third-party system who need a concrete pattern (anti-corruption layer) for protecting their own model from an upstream system's quirks.

## How it helps

- Gives the six standard context-mapping relationship patterns (shared kernel, customer/supplier, conformist, ACL, open host service, separate ways) with the specific signal for when to use each.
- Makes event storming with domain experts the starting point, not an engineer-only exercise reverse-engineered from existing code — so the map reflects the real domain, not accidental current coupling.
- Walks through a full worked example: an insurance company mapping Quoting, legacy PolicyCore, and Underwriting contexts, including a rejected "one unified Policy service" proposal and why an anti-corruption layer was needed against the legacy system.
- Explicitly connects the context map to downstream decisions — feeding into `microservices-decomposition` for service boundaries and into integration mechanism choices per relationship.

## How to use it

```bash
mkdir -p .claude/skills
cp -r domain-driven-design-context-mapping .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/domain-driven-design-context-mapping
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/domain-driven-design-context-mapping /path/to/your/project/.claude/skills/
```
