---
name: cost-aware-architecture-design
description: "Evaluates and designs architecture with cloud cost as a first-class constraint alongside latency, availability, and scalability — modeling cost-per-unit-of-work up front instead of discovering the bill after launch. Use when designing a new system, choosing between architectural options with different cost profiles, or responding to an unexplained cost spike."
---

# Cost-Aware Architecture Design

Treat cost as a first-class architectural NFR — modeled and traded off explicitly against latency, availability, and scalability during design — instead of an afterthought discovered in next month's cloud bill.

## When to use this

- A new system or feature is being designed and nobody has modeled its expected cost at projected scale before committing to the architecture.
- Two architectural options (serverless vs. always-on, managed service vs. self-hosted, single-region vs. multi-region) have meaningfully different cost profiles that haven't been compared explicitly.
- A cloud bill spiked unexpectedly and the root cause needs to be traced back to an architectural decision (over-provisioned capacity, an inefficient data-transfer pattern, an unbounded retry loop) rather than patched with a one-off budget alert.
- Finance or leadership is asking for cost-per-customer or cost-per-transaction figures that the current architecture makes difficult to even measure, let alone control.
- An architecture review (`architecture-review-board-prep`) requires a cost estimate as part of the design packet, and the team needs a structured way to produce one.

## Workflow

1. **Define the cost unit that matters for this system** — cost per request, per active user, per GB stored, per transaction — and make architectural decisions traceable to that unit rather than just an aggregate monthly bill that's hard to reason about at decision time.
2. **Model cost per component at projected scale**, not current scale: compute (always-on vs. serverless pricing curves cross over at a specific utilization point — model where that crossover sits for this workload), storage (hot vs. cold tiers), data transfer (especially cross-region and egress, often the most underestimated line item), and third-party/managed-service fees.
3. **Compare architectural options on cost curves, not single numbers.** Serverless is often cheaper at low/spiky utilization and more expensive than reserved always-on capacity past a certain sustained-utilization threshold — plot both options across the expected utilization range rather than comparing one snapshot cost.
4. **Attribute cost to the right owner** — tag/label resources by team, service, or customer so cost is visible and actionable at the level someone can actually control, not buried in one shared, unattributed cloud bill.
5. **Identify the highest-leverage cost levers before micro-optimizing**: right-sizing over-provisioned compute, moving cold data to cheaper storage tiers, reducing cross-region data transfer, and eliminating unbounded retry/fan-out patterns typically dwarf savings from smaller code-level tuning — start there.
6. **Model cost sensitivity to failure modes explicitly** — a retry storm, a runaway autoscaler, or an unbounded fan-out during an incident can spike cost as sharply as it spikes load; a resilience design (`circuit-breaker-pattern-design`, `rate-limiting-strategy-design`) is also a cost control, not just an availability one.
7. **Set a cost budget and alert threshold per system before launch**, tied to the projected cost-per-unit model, so a drift from the model (not just an absolute dollar spike) is caught early — a system whose cost-per-request creeps up 3x while traffic stays flat is a signal something architectural broke, even if the total bill still looks affordable.
8. **Weigh cost against the NFRs it trades against explicitly** (`nfr-to-architecture-mapping`) — cheaper single-region vs. costlier multi-region active-active is a real availability/cost trade-off that should be a documented decision, not a default nobody chose.
9. **Reassess cost assumptions at each significant scale milestone**, not just at design time — a cost model built for 10K users can become badly wrong (in either direction) at 1M users because of pricing-tier cliffs, reserved-capacity discounts, or newly-dominant cost drivers that didn't matter at the earlier scale.
10. **Feed cost data back into architecture decisions as a continuous loop**, not a one-time estimate — dashboard cost-per-unit alongside latency and error-rate dashboards so it's part of the same operational conversation, not a separate finance-only spreadsheet nobody on the engineering team sees.

## Example

Scenario: `StreamCast`, a video-processing platform, is deciding how to run its transcoding workload — currently a fleet of always-on EC2 instances, being considered against a move to serverless (AWS Lambda/Fargate-per-job) or a managed transcoding service.

Modeling:
- **Cost unit**: cost per video-minute transcoded.
- **Current (always-on)**: fleet sized for peak, sitting at ~35% average utilization; cost per video-minute ≈ $0.018, dominated by idle capacity during off-peak hours.
- **Option A (serverless per-job)**: no idle cost, but per-invocation pricing plus cold-start overhead; modeled cost per video-minute ≈ $0.009 at current (spiky, bursty) traffic pattern — cheaper because utilization is low and workload is bursty, which is exactly serverless's favorable zone.
- **Option B (managed transcoding service)**: flat per-minute rate, ≈ $0.014, but removes all operational burden and autoscaling risk.
- **Crossover analysis**: plotted against projected utilization growth, Option A remains cheaper than always-on until sustained utilization crosses ~65%, at which point reserved always-on capacity would become cheaper again — noted as a documented revisit trigger, not assumed permanent.
- **Failure-mode cost check**: a retry bug in the current system once caused a 40% cost spike in a single day from repeated re-transcoding of failed jobs; the new design adds a capped-retry policy (3 attempts, exponential backoff, dead-letter after) explicitly to bound this risk in the new architecture too.
- **Decision**: adopt Option A (serverless), with a cost-per-video-minute dashboard alert if it drifts >20% from the $0.009 baseline, and a revisit trigger at 65% sustained utilization.

Result: transcoding cost per video-minute drops ~50%, with an explicit, documented trigger for when the decision should be revisited rather than a silent assumption that serverless stays cheaper forever.

## Common pitfalls

- **Modeling cost at current scale only**, missing pricing-tier cliffs or utilization crossovers that flip the cheaper option as the system grows.
- **Comparing architectural options on a single cost snapshot** instead of a cost curve across the expected utilization range.
- **Underestimating data-transfer/egress cost**, often the largest unmodeled line item in a multi-region or multi-cloud design.
- **No cost attribution by team/service**, leaving cost as one shared number nobody feels accountable for or empowered to reduce.
- **Treating resilience patterns (retries, autoscaling) as availability-only concerns**, missing that their failure modes are also cost failure modes.
- **No budget or drift alert tied to the cost model**, so a cost regression is discovered a month later in the bill instead of the week it started.
- **One-time cost estimate never revisited**, going stale as scale, pricing, or workload shape changes.

## Output

A cost model doc (e.g., `docs/architecture/cost-model-<system>.md`) with the chosen cost unit, per-component cost projections at current and future scale, a cost-curve comparison across architectural options with a stated crossover point, a cost/NFR trade-off decision, and a budget + drift-alert threshold — feeding into the ADR and into ongoing cost dashboards.
