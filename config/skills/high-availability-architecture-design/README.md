# High Availability Architecture Design

A Claude Code skill for designing a system that stays up through routine failures — node crashes, AZ outages, dependency timeouts — by eliminating single points of failure and automating failover.

## Who it helps

- **Architects and tech leads** translating an uptime SLO into concrete redundancy decisions (N+1/N+2 sizing, replication mode, active-active vs active-passive).
- **Engineering managers** who need to justify the cost of redundancy against an actual downtime budget instead of an open-ended "make it more reliable" ask.
- **Developers** reviewing a design for hidden single points of failure before it ships, or debugging why an outage happened despite "having redundancy."

## How it helps

- Turns an availability target (e.g., 99.95%) into a concrete downtime budget and shows how to check a design against it using composed-availability math.
- Draws a hard line between high availability (automatic failover from routine failures, seconds) and disaster recovery (recovery from catastrophic loss, minutes-to-hours) so the two aren't conflated in design docs.
- Walks through a full worked example: a payment API's redundancy decisions across load balancer, app tier, database, and external dependency, each with a rejected alternative and why.
- Calls out the specific anti-patterns that quietly turn a "highly available" design into one with a hidden SPOF (single-AZ deployment, no automated failover, shared connection pools).

## How to use it

```bash
mkdir -p .claude/skills
cp -r high-availability-architecture-design .claude/skills/
```

Or place it under `~/.claude/skills/` for use across every project, or install it via a Claude Code plugin marketplace.

## Download just this skill

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/anilamar/software-architecture-skills.git
cd software-architecture-skills
git sparse-checkout set skills/high-availability-architecture-design
```

```bash
mkdir -p /path/to/your/project/.claude/skills
cp -r skills/high-availability-architecture-design /path/to/your/project/.claude/skills/
```
