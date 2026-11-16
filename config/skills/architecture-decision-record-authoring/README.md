# Architecture Decision Record Authoring

A Claude Code skill that writes clear, durable Architecture Decision Records (ADRs) — the short documents that capture *what* a team decided, *why*, and *what alternatives were rejected*, so the reasoning survives beyond the meeting it happened in.

## Who it helps

- **Tech leads and architects** who need to record consequential decisions (data store choice, service boundaries, API contracts) in a form the whole team can review and later reference.
- **Developers** who want a lightweight way to justify a design choice in a PR instead of leaving it undocumented in a commit message.
- **Engineering managers** who need a paper trail of technical decisions for audits, onboarding, or post-incident reviews.

## How it helps

- Turns a design discussion or review-board outcome into a structured, reviewable document instead of a diary entry.
- Forces the rejected alternatives to be written down — the single most valuable and most often-skipped part of a decision record.
- Standardizes status tracking (`Proposed` / `Accepted` / `Superseded` / `Deprecated`) so decisions can be traced over time.
- Includes a fully worked example (idempotency keys for a payments API) showing exactly what a good ADR looks like end to end.
- Reduces re-litigation of settled decisions during onboarding or team turnover.

## How to use it

As a Claude Code skill, drop this folder into a project so Claude can invoke it automatically when a decision needs recording:

```bash
mkdir -p .claude/skills
cp -r architecture-decision-record-authoring .claude/skills/
```

Or place it under `~/.claude/skills/` to make it available in every project on your machine. It can also be distributed to a team via a Claude Code plugin marketplace, where it's installed the same way any plugin skill is.

## Download just this skill

This skill lives inside the larger `software-architecture-skills` repo. Use a sparse checkout to fetch only this folder:

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/architecture-decision-record-authoring
```

Then copy it into your project:

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/architecture-decision-record-authoring /path/to/your/project/.claude/skills/
```
