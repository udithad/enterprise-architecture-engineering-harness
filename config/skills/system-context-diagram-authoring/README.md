# System Context Diagram Authoring

A Claude Code skill for drawing the highest-level view of a system — one box, its human actors, and the external systems it integrates with — to align stakeholders on scope before deeper design begins.

## Who it helps

- **Architects and tech leads** kicking off a new project or feature who need stakeholders sharing one mental model of scope before deeper design conversations start.
- **Engineering managers** resolving a boundary disagreement between teams (who owns what) with an explicit, agreed-on diagram instead of assumption.
- **Developers** onboarding onto an unfamiliar system who need the 30-second version of what it does and who it talks to, before diving into container- or component-level detail.

## How it helps

- Enforces the "one box, no internals" discipline that keeps a context diagram at the right altitude instead of collapsing into a container-level diagram.
- Requires every relationship to be labeled with what actually flows and in which direction, instead of vague, unlabeled arrows.
- Treats deliberately excluded integrations as an explicit callout, so readers know an omission was a decision, not an oversight.
- Walks through a full worked example: a healthcare scheduling platform's context diagram that resolved a real scope disagreement about billing ownership before design work began.

## How to use it

```bash
mkdir -p .claude/skills
cp -r system-context-diagram-authoring .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/system-context-diagram-authoring
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/system-context-diagram-authoring /path/to/your/project/.claude/skills/
```
