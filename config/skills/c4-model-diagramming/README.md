# C4 Model Diagramming

A Claude Code skill for producing Context, Container, Component, and Code diagrams at the right zoom level for the audience — instead of one overloaded diagram trying to answer every question about a system at once.

## Who it helps

- **Architects and tech leads** who need consistent, comparable diagrams across a design doc, ADR, or architecture review.
- **Engineering managers** who want onboarding and cross-team docs to have a diagram new engineers can actually navigate instead of a verbal walkthrough.
- **Developers** producing a diagram for a PR or design doc who need a clear rule for which level of detail (context, container, component, code) actually belongs.

## How it helps

- Walks through all four C4 levels and gives a clear rule for when each one earns its place — most systems only need Context and Container.
- Recommends text-based diagramming (Mermaid/PlantUML) so diagrams live in the repo and get reviewed alongside the code that would make them stale.
- Walks through a full worked example: a payments platform's Context, Container, and Component diagrams, including how the Component-level diagram becomes the centerpiece of a service-split ADR.
- Flags the common failure of one diagram trying to do all four jobs at once, and of diagrams pasted as images that silently drift from reality.

## How to use it

```bash
mkdir -p .claude/skills
cp -r c4-model-diagramming .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/c4-model-diagramming
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/c4-model-diagramming /path/to/your/project/.claude/skills/
```
