# Multi-Tenant Architecture Design

A Claude Code skill for choosing and enforcing a tenant-isolation model (silo, pool, or hybrid) for a multi-tenant SaaS system, based on tenant count, size variance, and compliance needs — instead of picking one model for the whole product regardless of tenant shape.

## Who it helps

- **Architects and tech leads** deciding between shared-schema, schema-per-tenant, and database-per-tenant models before the data model is locked in.
- **Engineering managers** who need to justify a tiered isolation strategy (why Enterprise gets a dedicated database and Free tier doesn't) to leadership or during a compliance review.
- **Developers** implementing or debugging tenant-scoping logic, especially after a near-miss cross-tenant data leak reveals that isolation was enforced only by convention.

## How it helps

- Frames isolation as a spectrum (silo / pool / hybrid) and gives concrete criteria — tenant count, size variance, compliance needs — for where a given tenant segment should sit on it.
- Insists isolation be enforced at the system level (database row-level security, unforgeable tenant context) rather than relying on every developer remembering a `WHERE tenant_id` clause.
- Covers the noisy-neighbor problem explicitly, with mitigation via per-tenant quotas and throttling.
- Walks through a full worked example — a B2B analytics SaaS with tiered isolation across 4,000 pooled tenants and 15 siloed enterprise tenants — including a documented tier-migration path and rejected alternatives.

## How to use it

```bash
mkdir -p .claude/skills
cp -r multi-tenant-architecture-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/multi-tenant-architecture-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/multi-tenant-architecture-design /path/to/your/project/.claude/skills/
```
