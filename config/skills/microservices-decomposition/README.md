# Microservices Decomposition

A Claude Code skill for splitting a monolith or greenfield system into microservices along bounded-context and data-ownership lines — instead of the common trap of splitting by technical layer.

## Who it helps

- **Architects and tech leads** designing service boundaries for a new system or planning an extraction from an existing monolith.
- **Engineering managers** who need a defensible rationale for why services are split the way they are, especially when justifying the plan to other teams or leadership.
- **Developers** who keep hitting cross-service coupling problems and want a concrete framework for proposing a better boundary.

## How it helps

- Gives a concrete checklist (team ownership, scaling profile, one-paragraph API contract test) for validating a candidate service boundary before committing to it.
- Uses change-coupling analysis (which files change together in commits) as a data-driven signal, not just org charts or intuition.
- Walks through a full worked example: decomposing an e-commerce monolith into Catalog, Cart, Checkout, Inventory, Shipping, and Billing services, including a rejected alternative split and why it was rejected.
- Explicitly calls out the "decomposing by technical layer" anti-pattern that produces a distributed monolith instead of real service independence.

## How to use it

```bash
mkdir -p .claude/skills
cp -r microservices-decomposition .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/microservices-decomposition
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/microservices-decomposition /path/to/your/project/.claude/skills/
```
