---
name: serverless-architecture-fit-assessment
description: "Assesses whether FaaS/serverless actually fits a given workload's traffic shape, latency needs, and cost curve, rather than adopting it by default trend. Use when choosing a compute model for a new workload, or reassessing an existing serverless workload whose cost or latency has become a concern."
---

# Serverless Architecture Fit Assessment

Evaluate whether a Function-as-a-Service/serverless compute model actually fits a specific workload's traffic pattern, latency requirements, and cost curve — rather than adopting serverless by default because it's trendy, or rejecting it by default because "it's not for us" — since serverless is a strong fit for some workload shapes and a poor one for others.

## When to use this

- A new workload is being designed and the team needs to choose between serverless (FaaS), containers, or traditional VMs/servers as the compute model.
- An existing serverless workload's cost has grown unexpectedly (a common surprise once traffic scales past the point where per-invocation pricing beats reserved capacity) and needs re-evaluation.
- A workload is hitting cold-start latency issues in production and the team needs to decide whether to mitigate (provisioned concurrency, warm-up pings) or migrate off serverless.
- A team is defaulting to serverless for every new service without evaluating fit, or conversely refusing to consider it despite a good fit (bursty, infrequent, event-driven workloads).

## Workflow

1. **Characterize the traffic pattern first**: steady/predictable high volume, bursty/spiky, or infrequent/sparse. Serverless's economic and operational advantages are strongest for bursty or infrequent workloads (pay only for actual invocations, scales to zero) and weakest for steady high-volume workloads (per-invocation pricing usually costs more than reserved capacity at sustained high utilization).
2. **Check latency sensitivity against cold-start reality** — if the workload has a strict low-latency requirement (e.g., sub-100ms) and can't tolerate occasional cold-start spikes (hundreds of ms to seconds, worse for some runtimes/languages), either serverless needs provisioned concurrency (which erodes much of the cost benefit) or a different compute model fits better.
3. **Evaluate execution duration and resource limits against the platform's constraints** — most FaaS platforms cap execution time (often 15 minutes) and memory/CPU; a long-running batch job or a workload needing sustained high CPU/GPU doesn't fit the FaaS execution model regardless of traffic shape, and belongs in `batch-processing-pipeline-design` territory instead.
4. **Assess statefulness** — FaaS functions are (by design) stateless between invocations; a workload needing in-memory state across requests (a stateful cache, a long-lived connection pool with expensive setup) either needs external state (adding latency/cost) or doesn't fit the model well.
5. **Model the actual cost curve, not just the per-invocation price** — compute per-invocation cost at current volume and at 3x/10x projected growth, and compare against the cost of an equivalent reserved-capacity (container/VM) deployment at the same volumes; serverless frequently wins at low/bursty volume and loses at sustained high volume, and the crossover point should be calculated, not assumed.
6. **Account for hidden operational costs of serverless**: vendor lock-in to a specific FaaS platform's programming model (see `vendor-lock-in-risk-assessment`), harder local testing/debugging, more complex observability across many short-lived function invocations, and orchestration complexity once a workflow spans many chained functions.
7. **Check the event-source fit** — serverless is a particularly strong fit when the workload is naturally event-driven (an object uploaded to storage, a message on a queue, an HTTP request) and a poor fit when it's naturally a long-running process or a workload requiring fine-grained control over the runtime environment.
8. **Decide per-workload, not per-system** — a system can legitimately mix serverless for bursty/event-driven parts and containers/VMs for steady-state, latency-critical, or long-running parts; don't force a single compute model system-wide if the workloads genuinely differ in shape.
9. **Pilot with real production-like traffic before committing** — cold-start behavior, cost, and third-party integration friction (DB connection limits under FaaS concurrency spikes, for instance) are easy to underestimate from documentation alone and are best validated with a load test resembling real traffic.
10. **Document the decision** as an ADR stating the traffic pattern, latency requirement, modeled cost curve at current and projected scale, and the chosen compute model with rationale — including for the parts of the system where serverless was deliberately rejected.

## Example

Scenario: `SnapReport`, a SaaS analytics tool, is deciding how to run two new workloads: (1) generating a PDF report on user demand (triggered a few thousand times a day, unevenly, each taking 2-8 seconds), and (2) a nightly aggregation job that processes the full dataset (runs for 45 minutes, high sustained CPU, once daily).

Assessment:
- **PDF generation**: bursty, infrequent (thousands/day but concentrated in short windows), short duration (well under FaaS time limits), naturally event-driven (triggered by an API call or queue message), stateless (each report generation is independent). Cost modeled: at current volume, FaaS costs ~$40/month vs. ~$300/month for a dedicated always-on container sized for peak. **Decision: FaaS is a strong fit.** Cold-start (~800ms on this runtime) is acceptable since report generation already takes 2-8 seconds — the cold-start is a small fraction of total latency, not a UX-breaking spike.
- **Nightly aggregation**: 45-minute duration exceeds the FaaS platform's 15-minute execution limit outright, ruling out plain FaaS regardless of cost. Sustained high CPU for the full 45 minutes doesn't benefit from a pay-per-invocation model anyway (it's the "steady load" case where reserved capacity is cheaper). **Decision: run as a scheduled batch job on a reserved container/VM**, not serverless — reusing `batch-processing-pipeline-design` for checkpointing and idempotent reprocessing.

Result: the two workloads' actual shapes drove two different compute decisions within the same system, both cost-modeled and stated in an ADR, rather than a single default choice applied to both.

## Common pitfalls

- **Adopting serverless by default trend without checking traffic shape**, ending up overpaying once a workload's volume becomes steady/high rather than bursty.
- **Rejecting serverless by default without evaluating fit**, missing real cost and operational savings on genuinely bursty, event-driven workloads.
- **Ignoring the platform's hard execution-time and resource limits**, discovering mid-implementation that a long-running job simply cannot run as a plain FaaS function.
- **Underestimating cold-start impact on a latency-sensitive path**, then discovering it in production rather than during evaluation.
- **Not modeling the cost curve at projected future scale**, locking into serverless during an early low-volume phase and hitting a cost surprise once volume grows past the crossover point.
- **Treating compute model as a system-wide binary choice** instead of per-workload, missing opportunities to mix serverless and reserved-capacity compute where each genuinely fits better.
- **No pilot/load test before committing**, missing real-world friction (connection pool exhaustion under concurrency spikes, third-party rate limits) that documentation doesn't surface.

## Output

A serverless-fit assessment doc (or ADR) per workload: traffic pattern classification, latency sensitivity vs. cold-start reality, execution-duration/resource fit against platform limits, a modeled cost curve at current and projected scale, and the resulting compute model decision with rationale — including workloads where serverless was deliberately not chosen.
