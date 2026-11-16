# Data Lake vs. Warehouse Architecture Choice

A Claude Code skill for deciding between (or combining) a data lake and a data warehouse based on data shape, schema stability, and consumer query patterns — instead of defaulting to whichever pattern is trendy or already in place.

## Who it helps

- **Architects and tech leads** designing analytical infrastructure from scratch, or untangling a warehouse being misused for raw/evolving data or a lake that's become an ungoverned swamp.
- **Engineering managers** balancing competing consumer needs — data scientists wanting flexible raw access, analysts wanting governed BI tables — through one platform decision.
- **Developers** building ETL/ELT pipelines who need a clear landing-zone and promotion path (bronze/silver/gold) for new data sources instead of re-litigating the choice every time.

## How it helps

- Characterizes data by shape and change rate, and consumers by query pattern, before recommending a pattern — rather than treating lake-vs-warehouse as an ideological choice.
- Introduces the layered bronze/silver/gold structure that applies whether the underlying choice is a pure lake, pure warehouse, or lakehouse.
- Walks through a full worked example: a warehouse being misused for raw clickstream data, redesigned into a lakehouse that cuts warehouse cost ~40% and isolates upstream schema breaks to the bronze layer.
- Covers governance and access control gaps that a lake needs explicitly, since schema-on-read doesn't come with the guarantees a warehouse provides by default.

## How to use it

```bash
mkdir -p .claude/skills
cp -r data-lake-vs-warehouse-architecture-choice .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/data-lake-vs-warehouse-architecture-choice
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/data-lake-vs-warehouse-architecture-choice /path/to/your/project/.claude/skills/
```
