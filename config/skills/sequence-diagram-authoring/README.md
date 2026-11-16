# Sequence Diagram Authoring

A Claude Code skill for producing sequence diagrams that precisely model request/response and message flows across components over time — order of calls, sync vs. async, and failure/alternative paths — instead of ambiguous prose.

## Who it helps

- **Architects and tech leads** documenting a saga, API interaction, or event chain precisely enough to review ordering bugs and missing error handling before implementation.
- **Developers** onboarding onto an unfamiliar multi-service flow who need to understand call order and blocking behavior without reading every service's code.
- **Engineering managers** who need a design review artifact that shows exactly how a decision plays out at the interaction level, not just at the component level.

## How it helps

- Insists on distinguishing synchronous calls from asynchronous messages explicitly — often the single most important fact a sequence diagram needs to convey.
- Requires modeling at least one failure/alternative path, not just the happy path, since that's usually the part most worth reviewing.
- Gives a concrete rule for scope (one scenario per diagram) that keeps diagrams readable instead of trying to show an entire system at once.
- Walks through a full worked example: a Mermaid sequence diagram for a booking saga's happy path and payment-failure compensation path, with stated preconditions and postconditions.

## How to use it

```bash
mkdir -p .claude/skills
cp -r sequence-diagram-authoring .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/sequence-diagram-authoring
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/sequence-diagram-authoring /path/to/your/project/.claude/skills/
```
