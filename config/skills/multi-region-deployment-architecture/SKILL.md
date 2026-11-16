---
name: multi-region-deployment-architecture
description: "Designs active-active or active-passive multi-region deployments with explicit data replication, failover, and traffic-routing strategy. Use when a system must survive a full region outage, serve users with low latency across geographies, or meet a regulatory data-residency requirement."
---

# Multi-Region Deployment Architecture

Design a system to run across two or more geographic regions — deliberately choosing active-active or active-passive, a data replication strategy, and a traffic-routing mechanism — so a region-level outage, latency requirement, or data-residency rule is handled by design rather than by improvisation during an incident.

## When to use this

- A system's availability SLO can no longer be met by a single region (a regional cloud outage would take the whole product down) and `high-availability-architecture-design` within one region isn't sufficient.
- Users are geographically distributed and single-region latency is hurting UX or failing a latency-based SLA.
- A regulatory or contractual requirement (GDPR, data residency laws, an enterprise customer contract) mandates that certain data stay within a specific geography.
- `disaster-recovery-architecture-planning` has set an RTO/RPO that a single-region backup/restore strategy cannot meet, forcing a live secondary region.
- A postmortem for a regional outage recommends multi-region as a structural fix rather than a one-off patch.

## Workflow

1. **Choose active-active or active-passive deliberately**, per service or data domain, not as one blanket system-wide choice. Active-active (all regions serve live traffic) gives lower latency and no failover delay but requires solving multi-region write conflicts. Active-passive (one region serves traffic, another stands by) is simpler operationally but wastes standby capacity and has a failover gap.
2. **Classify data by residency and consistency needs first** (reuse `data-consistency-model-selection`): what must stay in-region for compliance, what can replicate freely, and what needs strong consistency across regions versus what tolerates cross-region staleness.
3. **Pick a replication topology per data store**: single-leader with cross-region followers (simple, but writes bottleneck through one region and failover has a gap), multi-leader with conflict resolution (enables active-active writes, requires CRDTs or app-level merge), or leaderless quorum (tunable consistency, higher write latency).
4. **Design traffic routing explicitly**: DNS-based geo-routing (simple, slow to react to failure — TTL and client caching delay convergence), anycast or a global load balancer (faster failover, more infrastructure investment), or client-side region selection with health-aware fallback.
5. **Define the failover trigger and mechanism precisely**: what health signal triggers failover (region-level health check, not a single service), whether failover is automatic or requires a human decision, and the expected time-to-detect plus time-to-reroute — this sum is the real RTO, not just "failover takes 30 seconds."
6. **Handle in-flight requests and sessions during failover** — sticky sessions pinned to a now-dead region need a defined fallback (re-authenticate, replay from a shared session store, or accept the dropped session), stated explicitly rather than left as an unspecified edge case.
7. **Test failover as a routine exercise, not a hypothetical** — schedule game days that actually redirect production traffic away from a region, since untested failover paths reliably have hidden breakage (stale config, hardcoded region endpoints, missing IAM permissions in the standby region).
8. **Account for the cost and operational overhead of the standby/secondary region explicitly** — an active-passive standby that's never load-tested at full traffic is a false sense of security; size and test it to actually absorb full production load.
9. **Version and replicate configuration and secrets across regions**, not just data — a common failure mode is a fully replicated database but a secondary region running stale application config or missing feature flags.
10. **Document the chosen architecture as an ADR** stating per-data-domain replication strategy, routing mechanism, failover trigger, and measured (not assumed) RTO/RPO.

## Example

Scenario: `GlobalCart`, an e-commerce platform, currently runs single-region in `us-east-1`. A regional AWS outage caused six hours of downtime, and the company has customers in the EU subject to GDPR data-residency requirements.

Decision, per data domain:
- **Product catalog** (read-heavy, no residency constraint): active-active across `us-east-1` and `eu-west-1`, read replicas in both regions, writes go through a single-leader in `us-east-1` with async replication to `eu-west-1` (catalog writes are infrequent and centrally managed, so single-leader is acceptable; a few seconds of cross-region staleness on new listings is tolerable).
- **EU customer PII and order data**: must stay in-region for GDPR. Deployed as a fully separate regional stack in `eu-west-1` with its own database, not merely a replica — EU users are routed to `eu-west-1` exclusively via geo-DNS, and no PII replicates cross-region.
- **Shopping cart** (write-heavy, per-user, latency-sensitive): active-active with a leaderless, per-user-partitioned store; each user's cart writes land in whichever region they're closest to, with last-write-wins conflict resolution (acceptable since concurrent same-user cart edits from two regions are rare and low-stakes).
- **Traffic routing**: a global anycast load balancer routes by latency for non-EU users, and by hard geo-rule for EU users (compliance overrides latency-optimal routing for that segment).
- **Failover**: automated for the catalog and cart services (health-check-driven, sub-minute), but the initial cutover of EU infrastructure to a second EU region (for DR within the EU) is a manual, on-call-triggered runbook rather than automatic, since data-residency correctness during a rushed automatic failover was judged too risky to leave unattended.

Result: a full `us-east-1` outage now degrades EU customers not at all (fully separate stack) and degrades non-EU customers to reduced catalog freshness and normal cart function, instead of a full six-hour outage. The design doc explicitly states measured RTO (90 seconds, from three tested game days) rather than a theoretical estimate.

## Common pitfalls

- **Choosing active-active by default for its latency benefits without solving the write-conflict problem it creates** — leads to silent data loss or corruption under concurrent cross-region writes.
- **Replicating data but not configuration/secrets**, so failover reveals a standby region running stale code paths or missing flags.
- **Never load-testing the standby region at full production traffic**, discovering under a real outage that it can't actually absorb the load.
- **Treating DNS TTL as instant** — DNS-based failover has a real propagation delay that must be included in the stated RTO, not ignored.
- **Leaving session/in-flight-request handling on failover unspecified**, causing silent request drops or confusing partial failures during a real cutover.
- **Applying one residency/consistency answer to the whole system** instead of classifying data domain by domain, over- or under-engineering different parts.
- **Never rehearsing failover in production-like conditions**, so the first real failover is also the first time hidden breakage (hardcoded endpoints, missing permissions) is discovered.

## Output

A multi-region architecture decision doc (or ADR) stating, per data domain: active-active vs. active-passive, replication topology and conflict resolution, residency constraints, the traffic-routing mechanism, the failover trigger and measured RTO/RPO, and a scheduled game-day/failover-testing cadence.
