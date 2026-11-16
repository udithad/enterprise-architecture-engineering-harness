---
name: capacity-planning-and-scaling-model
description: "Models expected load growth and translates it into concrete scaling triggers, resource headroom targets, and a scaling plan per tier, so capacity decisions are made from data instead of reacting to an outage. Use when planning for a known growth event, or when a system's current headroom is unknown."
---

# Capacity Planning and Scaling Model

Turn expected load growth into a concrete model — current headroom per tier, the load level at which each tier breaks, and the specific trigger and action to scale before it does — instead of discovering capacity limits during an outage or a marketing-driven traffic spike.

## When to use this

- A known growth event is coming (product launch, marketing campaign, seasonal peak, a large new customer onboarding) and nobody has modeled whether current capacity can absorb it.
- Autoscaling exists but nobody has validated its trigger thresholds against real breaking points — it might scale too late (after user-facing errors start) or too aggressively (burning budget on false-positive scale-outs).
- A recent incident traced back to a resource limit (connection pool exhaustion, disk fill, thread pool saturation) that nobody had modeled or was tracking headroom against.
- Leadership is asking "how much runway do we have before we need to scale infrastructure/budget" and the honest answer is "we don't know."
- A new service is being designed and its capacity/scaling story needs to be defined before launch, not retrofitted after the first traffic spike.

## Workflow

1. **Establish the current baseline load and its resource footprint per tier** — requests/sec, DB connections in use, queue depth, CPU/memory per instance — measured, not estimated, across a normal week including peak hours.
2. **Identify the binding constraint per tier**, i.e., what actually runs out first as load increases: CPU, memory, DB connections, thread pool size, disk I/O, a rate limit on a downstream dependency, or a hard quota (see `resource-contention-diagnosis` for tracing an existing bottleneck). Different tiers usually have different binding constraints — model each one, not just the most obvious.
3. **Model growth scenarios explicitly**: organic trend (current MoM growth rate projected forward), a specific known event (a marketing campaign expecting 5x traffic for 2 hours), and a stress scenario (worst case you want to survive without falling over, even if degraded). Each needs its own capacity answer.
4. **Load-test toward the actual breaking point**, not just toward the expected peak — knowing a service degrades gracefully at 3x and falls over hard at 4x is far more useful than only confirming it survives at 1.5x expected peak (see `capacity-testing-and-headroom-planning`).
5. **Translate headroom into a concrete scaling trigger per tier**: "scale out application pods when CPU > 65% sustained for 3 minutes" or "add a read replica when replication lag-adjusted read QPS exceeds 70% of a single replica's tested capacity" — vague targets like "scale when it gets slow" don't translate into an actual autoscaling policy (see `autoscaling-policy-design`).
6. **Set triggers with enough lead time to act before user impact**, accounting for how long scaling actually takes — a database read replica that takes 20 minutes to provision needs a trigger fired well before the primary is saturated, not at the moment it is.
7. **Identify the tiers that can't autoscale and need pre-provisioned headroom** — a primary database, a licensed third-party dependency with a fixed quota, a stateful cache cluster — and set an explicit manual capacity-review cadence for these instead of assuming elasticity that doesn't exist.
8. **Model cost against each scaling scenario** (see `cost-aware-architecture-design`) so scaling for a known event has a budget attached before it happens, not a surprise bill after.
9. **Write a scaling runbook for the known-event scenario**: pre-scale steps to take before the event (warm caches, pre-provision extra capacity, notify on-call), and rollback steps to scale back down after, so temporary headroom doesn't become a permanent cost.
10. **Revisit the model on a regular cadence and after every relevant incident** — capacity headroom erodes as organic growth continues, and a model validated 8 months ago against last year's traffic pattern is not a safe basis for this year's peak.

## Example

Scenario: `TicketNest`, an event-ticketing platform, has a major on-sale event coming where 200,000 users are expected to attempt purchase within a 10-minute window — roughly 40x normal peak traffic.

Modeling:
- **Baseline**: normal peak is 500 req/sec, DB connection pool at 40% utilization, application tier at 3 pods.
- **Binding constraints identified**: (1) DB connection pool (hard limit 200 connections, currently 80 used at peak), (2) payment-gateway rate limit (fixed 1,000 req/sec contractual cap), (3) application pod CPU.
- **Load test to breaking point**: simulated 40x traffic; application tier autoscaled cleanly to 45 pods; DB connection pool exhausted at 12x normal peak, well before the expected 40x — confirmed as the actual binding constraint, not CPU.
- **Mitigation before the event**: introduce a connection-pool-aware queue (a virtual waiting room) that admits requests to checkout at a rate the DB and payment gateway can sustain, decoupling "users who want to buy" from "requests the backend can serve" — turning a hard capacity wall into a bounded queue with a visible wait time instead of 5xx errors.
- **Scaling trigger**: application tier autoscales on CPU > 60%; DB connections monitored with an alert at 70% of pool capacity feeding a runbook step to throttle the admission queue's release rate.
- **Runbook**: pre-scale application tier to 20 pods 30 minutes before on-sale start (avoids autoscaler lag during the initial spike), on-call staffed for the 10-minute window, scale back to 3 pods 2 hours after.
- **Cost**: modeled at ~$180 for the event's extra compute, approved in advance.

Result: on-sale traffic is admitted through the queue at a rate the DB and payment gateway can sustain; no 5xx errors; average admitted-user wait time 45 seconds instead of a wall of failed purchase attempts.

## Common pitfalls

- **Load-testing only to the expected peak**, never learning where the system actually breaks or how gracefully (or not) it degrades past that point.
- **Assuming every tier autoscales**, when a primary database, a third-party rate limit, or a stateful component often can't and needs a different plan entirely.
- **Scaling triggers set too late**, especially for tiers with slow provisioning time (databases, VMs vs. containers), so the trigger fires but capacity doesn't arrive before user impact.
- **No cost estimate attached to a scaling scenario**, turning a planned traffic event into a surprise bill.
- **Treating a capacity model as a one-time exercise** instead of revisiting it as organic growth erodes previously-safe headroom.
- **Modeling only the obvious constraint (CPU)** while an unmodeled one (connection pool, third-party rate limit) is what actually breaks first under load.

## Output

A capacity plan (e.g., `docs/architecture/capacity-plan-<event-or-system>.md`) with the current baseline and binding constraint per tier, load-test results toward the actual breaking point, concrete scaling triggers with lead-time justification, a cost estimate per scenario, and a runbook for any known upcoming event — feeding into autoscaling policy configuration and pre-event on-call planning.
