# Disaster Recovery Architecture Planning

A Claude Code skill for setting quantified RTO/RPO targets per system tier and designing the backup, replication, and failover architecture to actually meet them, instead of assuming redundancy within a region is the same thing as a tested disaster recovery plan.

## Who it helps

- **Architects and tech leads** choosing between cold, pilot-light, warm-standby, and active-active postures per tier, and designing backups that survive ransomware or insider deletion, not just node failure.
- **Engineering managers** who need to answer an auditor's or leadership's "how long would it take us to recover" question with a tested number instead of a guess.
- **Developers** implementing the failover runbook, cross-region replication, or immutable backup pipeline for a specific tier.

## How it helps

- Separates disaster recovery from high availability explicitly, and separates "replica" from "backup" — a distinction most incident postmortems reveal was missing.
- Gives a concrete per-tier RTO/RPO worked example (`Northwind Payments`) with a posture, backup design, and failover runbook sized to what each tier actually needs, plus a rejected active-active alternative and why it was overkill.
- Insists on scheduled, measured failover testing (game days) rather than a runbook that has never been executed — the single most common reason DR plans fail during a real incident.
- Cross-references `high-availability-architecture-design`, `multi-region-deployment-architecture`, and `data-consistency-model-selection` so this skill stays focused on recovery from a disaster rather than duplicating availability or replication-topology concerns.

## How to use it

```bash
mkdir -p .claude/skills
cp -r disaster-recovery-architecture-planning .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/disaster-recovery-architecture-planning
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/disaster-recovery-architecture-planning /path/to/your/project/.claude/skills/
```
