# Multi-Region Deployment Architecture

A Claude Code skill for designing active-active or active-passive deployments across multiple geographic regions — with explicit data replication, traffic routing, and failover strategy — so a region outage, latency requirement, or data-residency rule is handled by design.

## Who it helps

- **Architects and tech leads** deciding between active-active and active-passive per data domain, and designing the replication and routing that choice requires.
- **Engineering managers** who need a system's real (tested, not assumed) RTO stated and defensible after a regional outage or in front of an enterprise customer's compliance review.
- **Developers** implementing cross-region data replication or session handling during failover, who need a concrete pattern for conflict resolution and residency constraints.

## How it helps

- Frames the core decision — active-active vs. active-passive, and replication topology — per data domain rather than as one system-wide default.
- Distinguishes real failover time (detection + reroute, tested) from a theoretical "failover takes 30 seconds" estimate.
- Walks through a full worked example: an e-commerce platform splitting catalog, EU customer data, and shopping cart into three different multi-region strategies based on their actual consistency and residency needs.
- Covers the operational discipline (replicated config/secrets, scheduled game days) that most multi-region designs skip and then fail on during a real outage.

## How to use it

```bash
mkdir -p .claude/skills
cp -r multi-region-deployment-architecture .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/multi-region-deployment-architecture
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/multi-region-deployment-architecture /path/to/your/project/.claude/skills/
```
