# Database-Per-Service Design

A Claude Code skill for giving each service exclusive ownership of its own database, with every cross-service data need routed through an API or an event, instead of services sharing tables and becoming a distributed monolith with extra network hops.

## Who it helps

- **Architects and tech leads** deciding which table belongs to which service during a decomposition, and how to replace cross-table joins with an API call, an event-driven local projection, or a deliberate move to a data warehouse.
- **Engineering managers** trying to stop a service's schema migration from being blocked by another team's direct queries against its tables.
- **Developers** building the first service extraction who need a concrete plan for what a foreign key becomes once it crosses a database boundary.

## How it helps

- Gives a step-by-step method for assigning table ownership, replacing cross-service joins, and deciding what a foreign key becomes once it's no longer enforceable by one database.
- Walks through a full worked example: splitting `ShopMono`'s shared Postgres database across Checkout, Inventory, Catalog, and Shipping services, including which access patterns become synchronous API calls versus event-driven local projections.
- Calls out the migration sequencing explicitly — introduce a service-owned API in front of shared tables first, migrate consumers, then physically split the database — instead of a risky big-bang cutover.
- Cross-references `saga-pattern-for-distributed-transactions` for multi-service transactions and `data-lake-vs-warehouse-architecture-choice` for reporting access, so this skill stays focused on data ownership rather than duplicating those concerns.

## How to use it

```bash
mkdir -p .claude/skills
cp -r database-per-service-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/database-per-service-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/database-per-service-design /path/to/your/project/.claude/skills/
```
