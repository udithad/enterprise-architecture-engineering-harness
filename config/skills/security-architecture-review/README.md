# Security Architecture Review

A Claude Code skill for systematically reviewing a system's architecture — not its code — for authentication/authorization gaps, trust-boundary violations, and sensitive-data exposure paths, catching structural security flaws before they ship.

## Who it helps

- **Architects and tech leads** who need a repeatable method for walking trust boundaries and sensitive-data flows before a security-sensitive launch, instead of an ad hoc gut check.
- **Engineering managers** preparing evidence of security posture for a review board, compliance audit, or enterprise customer, with findings ranked by severity and assigned an owner.
- **Developers** designing service-to-service integrations who need a concrete checklist for the confused-deputy problem, credential scoping, and blast-radius minimization.

## How it helps

- Separates authentication review from authorization review — the two most commonly conflated, and most commonly incompletely checked, security properties.
- Gives a concrete method for tracing sensitive data end to end, including the commonly-missed paths (logs, caches, backups) that a "is the database encrypted" check skips.
- Names the confused deputy pattern explicitly and shows how to spot it in a service-to-service architecture.
- Walks through a full worked example: a healthcare scheduling platform's security review of a new partner billing API, finding and remediating five distinct architectural gaps before a HIPAA-relevant launch.

## How to use it

```bash
mkdir -p .claude/skills
cp -r security-architecture-review .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/security-architecture-review
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/security-architecture-review /path/to/your/project/.claude/skills/
```
