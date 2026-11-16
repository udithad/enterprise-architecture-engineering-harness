# Hexagonal Architecture Design (Ports & Adapters)

A Claude Code skill for isolating business logic behind ports and adapters so the domain core stays independent of frameworks, databases, and transport technology — and stays fast to unit test.

## Who it helps

- **Developers** who want to unit test business logic without spinning up a real database or external service for every test.
- **Architects** planning for technology swaps (REST to gRPC, Postgres to DynamoDB, Kafka to SQS) who want the change contained to one adapter.
- **Tech leads** who need multiple entry points (HTTP API, CLI, batch job, message consumer) to share one implementation of a use case without duplicating logic.

## How it helps

- Provides a concrete ports-and-adapters directory layout with driving adapters (HTTP, CLI, batch) and driven adapters (database, payment processor, clock).
- Walks through a full worked example: a subscription-renewal core shared by an HTTP API and a nightly batch job, tested with in-memory fakes instead of real infrastructure.
- Distinguishes ports that genuinely decouple the core from ones that just mirror a library's API (a common false-decoupling mistake).
- Recommends contract tests so real and fake adapters don't silently drift apart.

## How to use it

Copy the skill folder into your project's Claude Code skills directory:

```bash
mkdir -p .claude/skills
cp -r hexagonal-architecture-design .claude/skills/
```

Or place it under `~/.claude/skills/` for global availability, or install it through a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/hexagonal-architecture-design
```

Then copy it into your project:

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/hexagonal-architecture-design /path/to/your/project/.claude/skills/
```
