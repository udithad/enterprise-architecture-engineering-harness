# Layered Architecture Design

A Claude Code skill for structuring a system into presentation, application, domain, and infrastructure layers with a strict, one-way dependency rule — the classic remedy for codebases where business logic, database access, and HTTP handling have all blurred together.

## Who it helps

- **Developers** setting up a new service or refactoring an existing one who need a concrete, opinionated starting structure instead of guessing where code belongs.
- **Tech leads** establishing conventions for a team so code review has an objective standard ("does this violate the dependency rule?") instead of subjective taste.
- **Architects** planning a future service extraction — clean layering is what makes pulling a module out later mechanical rather than a rewrite.

## How it helps

- Gives a concrete four-layer structure (presentation / application / domain / infrastructure) with an explicit, enforceable dependency direction.
- Shows a full worked example — a Flask + SQLAlchemy order service refactored from tangled route handlers into clean layers with dependency-inverted interfaces.
- Flags the specific code smells that signal layer leakage (ORM entities doubling as domain objects, business rules hiding in controllers).
- Points to automated enforcement (import-linters, ArchUnit-style fitness functions) so the layering survives past the initial refactor.

## How to use it

Copy the skill into your project so Claude Code can apply it when scaffolding or refactoring a service:

```bash
mkdir -p .claude/skills
cp -r layered-architecture-design .claude/skills/
```

Or install it under `~/.claude/skills/` for use across all your projects, or via a Claude Code plugin marketplace if your team distributes skills that way.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/layered-architecture-design
```

Then copy it into your project:

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/layered-architecture-design /path/to/your/project/.claude/skills/
```
