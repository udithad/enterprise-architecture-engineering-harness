# Observability Architecture Design

A Claude Code skill for architecting logging, metrics, and distributed tracing as one coherent system — with consistent correlation IDs and deliberate cardinality control — so engineers can diagnose unfamiliar production failures quickly instead of observability being bolted on incident by incident.

## Who it helps

- **Architects and tech leads** designing observability into a new system from the start, or auditing an existing system's diagnostic blind spots after a slow incident.
- **Engineering managers** who need incident diagnosis time reduced structurally, not by hoping the next on-call engineer happens to know where to look.
- **Developers** instrumenting services who need a concrete rule for what goes in a metric label versus a log field versus a trace span, to avoid cardinality blowups and unqueryable logs.

## How it helps

- Separates the distinct jobs of metrics, logs, and traces so each pillar is used for what it's actually good at, instead of one pillar overloaded to do all three.
- Gives a concrete cardinality rule (never put unbounded values in metric labels) that prevents the single most common cause of a metrics system becoming slow or unaffordable.
- Distinguishes symptom-based alerting (user-visible, low noise) from cause-based alerting (high noise, alert fatigue).
- Walks through a full worked example: a logistics platform redesigning cross-service correlation, structured logs, RED metrics, and tracing after a four-hour diagnosis incident, validated with a follow-up game day.

## How to use it

```bash
mkdir -p .claude/skills
cp -r observability-architecture-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/observability-architecture-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/observability-architecture-design /path/to/your/project/.claude/skills/
```
