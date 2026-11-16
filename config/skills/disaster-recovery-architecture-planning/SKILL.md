---
name: disaster-recovery-architecture-planning
description: "Defines per-tier RTO/RPO targets and designs the backup, replication, and failover architecture needed to actually meet them when a whole region, data store, or environment is lost — not just a single node. Use when a system has never had its recovery time quantified, after a near-miss outage, or as a compliance requirement (SOC 2, ISO 27001, banking regulation)."
---

# Disaster Recovery Architecture Planning

Design for the disaster that redundancy didn't prevent: a region going dark, a corrupted database silently replicated to every replica, a ransomware encryption event, or an entire cloud account being deleted. High availability (see `high-availability-architecture-design`) is about absorbing failures without an outage; disaster recovery is about having a tested, bounded-time path back to a working system once HA's assumptions have already failed. A DR plan that has never been tested with a real failover is not a DR plan — it's a hope.

## When to use this

- A system has redundancy and failover within a region but no answer to "what happens if this entire region/data-center becomes unavailable."
- An auditor, customer contract, or regulation (SOC 2, ISO 27001, PCI-DSS, banking/insurance regulators) requires documented and tested RTO/RPO targets.
- A near-miss (a region had a multi-hour outage elsewhere and it made leadership ask "could that happen to us") triggers a review of actual recovery capability.
- A database corruption or bad migration is discovered days after it happened, already propagated to every read replica, and the org realizes replication is not a substitute for point-in-time recovery.
- A ransomware or insider-deletion scenario needs to be planned for, where an attacker with production credentials could also delete backups if they're reachable from the same trust boundary.
- Leadership asks "how long would it take us to recover from losing us-east-1 entirely" and nobody can answer with a number backed by a test.

## Workflow

1. **Set RTO and RPO per system tier, not one number for the whole company.** RTO (Recovery Time Objective — how long can it be down) and RPO (Recovery Point Objective — how much data can be lost) differ wildly between a billing ledger (RPO near zero) and an internal analytics dashboard (RPO of a day is fine). Tier systems (e.g., Tier 0/1/2/3) and assign both numbers per tier, driven by actual business impact, not aspiration.
2. **Classify the disaster scenarios being planned for**, because each demands a different architecture: region loss (infrastructure-level), data corruption/bad deploy (needs point-in-time recovery, not just a live replica), and malicious deletion/ransomware (needs immutable, access-isolated backups). A plan that only covers region loss leaves the other two unaddressed.
3. **Choose active-passive, pilot-light, warm-standby, or active-active per tier based on the RTO it must hit.** Active-passive (cold standby, restore from backup) can take hours; pilot-light (minimal always-on skeleton, scaled up on failover) cuts that to tens of minutes; warm-standby (fully deployed, scaled down) gets to minutes; active-active (see `multi-region-deployment-architecture`) gets to near-zero but costs the most and adds consistency complexity. Don't default to active-active for a tier that doesn't need sub-minute RTO — it's the most expensive option per tier by a wide margin.
4. **Design backups to be immutable and access-isolated from production credentials**, specifically to survive the ransomware/insider-deletion scenario. If the same IAM role that can write to production can also delete the backup bucket/snapshots, the backup doesn't protect against that scenario — use a separate account/vault with write-once-read-many (WORM) storage or a time-delayed deletion policy.
5. **Separate replication (for availability) from backup (for recoverability) explicitly.** A synchronous or near-synchronous replica protects against node/AZ failure but faithfully replicates corruption and accidental deletes within seconds — it is not a backup. Maintain point-in-time-recoverable snapshots (e.g., WAL-based PITR, periodic full + incremental backups) with a retention window that covers "how long could corruption go undetected."
6. **Design and document the failover runbook as an executable procedure**, not prose: DNS/traffic cutover steps, database promotion steps, the order services must come up in (respecting dependency graphs), and the explicit decision authority for declaring a disaster and initiating failover.
7. **Test failover on a schedule, not only in theory.** Run a real (or realistic simulated) failover at least annually, ideally via game days, and measure actual RTO/RPO achieved against the targets — an untested runbook reliably fails at the exact steps nobody rehearsed, usually a manual DNS change or a forgotten downstream dependency.
8. **Plan data consistency and reconciliation for the failback path**, not just failover. If the primary region comes back after a failover, writes that happened in the DR region during the outage must be reconciled or replayed back — decide this before the disaster, because doing it live under pressure is how data gets lost or duplicated.
9. **Account for dependency chains outside your own infrastructure** — DNS providers, certificate authorities, third-party payment/identity providers, and your own CI/CD pipeline (which may itself be needed to redeploy during recovery) can each become the actual bottleneck in a "region is down" scenario even when your application architecture is fine.
10. **Cost the plan explicitly per tier** and get it signed off — warm-standby and active-active DR roughly double (or more) infrastructure spend for the protected tier; this is a deliberate business trade-off against the cost of an extended outage, not something to leave implicit (see `cost-aware-architecture-design`).
11. **Feed DR requirements into the deployment and migration design** so schema/deployment practices don't quietly break the DR story — e.g., a zero-downtime migration (see `zero-downtime-deployment-architecture`) that assumes only one region is being written to needs re-checking if DR is active-active.

## Example

Scenario: `Northwind Payments` processes card transactions and settlement batches. Leadership asks for a documented DR posture ahead of a SOC 2 audit; today, Postgres has one synchronous standby in the same region and nightly `pg_dump` backups retained for 7 days in the same AWS account.

Tiering and targets:

| Tier | System | RTO | RPO |
|---|---|---|---|
| 0 | Transaction ledger (Postgres) | 15 min | 0 (zero data loss) |
| 1 | Settlement batch processor | 2 hours | 15 min |
| 2 | Merchant-facing dashboard | 4 hours | 1 hour |
| 3 | Internal reporting | 24 hours | 24 hours |

Redesign for Tier 0: cross-region streaming replication to a warm-standby Postgres in a second region (RTO target of 15 minutes requires the standby already provisioned and receiving WAL, not a cold restore); continuous WAL archiving to an immutable, cross-account S3 bucket with a 35-day PITR window and a separate IAM boundary that production's application role cannot delete from; a documented runbook: promote standby, flip Route 53 failover record, redeploy application tier pointed at new primary, verify settlement queue drained before resuming batch processing.

Rejected alternative: active-active across both regions for the ledger. Rejected because Tier 0's actual requirement is "recover within 15 minutes with zero loss," which warm-standby with synchronous cross-region replication meets; active-active would require solving write-conflict resolution for a financial ledger (a much harder and riskier problem — see `data-consistency-model-selection`) for no RTO/RPO benefit this tier actually needs.

Game day: a quarterly scheduled failover test promotes the standby in a maintenance window, measures actual time-to-serving-traffic (target: under 15 minutes), and reconciles any writes that landed on the old primary in a race window before DNS fully propagated.

## Common pitfalls

- **Confusing a replica with a backup.** A live replica faithfully copies corruption and accidental deletes within seconds; only point-in-time-recoverable, immutable backups protect against that class of disaster.
- **Backups reachable by the same credentials as production.** If a compromised or malicious production role can delete the backup too, the backup doesn't protect against the ransomware/insider scenario it's meant to cover.
- **One RTO/RPO number for the whole company** instead of per-tier targets — this either overspends on low-value systems or underprotects the ones that actually matter.
- **A runbook that has never been executed.** Untested failover procedures reliably break at the step nobody rehearsed — usually DNS TTLs, a forgotten dependency, or credentials that expired since the doc was written.
- **No failback plan.** Teams plan the failover and stop there, then improvise data reconciliation under pressure when the primary region recovers.
- **Ignoring third-party and tooling dependencies** — DNS, CI/CD, certificate renewal, and external APIs can each be the actual bottleneck in a real regional outage even when your own services are ready.
- **Defaulting to the most expensive DR posture (active-active) everywhere** instead of matching posture to each tier's actual RTO/RPO requirement and cost tolerance.

## Output

A disaster recovery plan (e.g., `docs/architecture/disaster-recovery-plan.md`) with per-tier RTO/RPO targets, the chosen posture (cold/pilot-light/warm-standby/active-active) and architecture per tier, the backup immutability and retention design, an executable failover/failback runbook, and a game-day testing schedule with results — feeding into the audit/compliance record and into on-call incident procedures.
