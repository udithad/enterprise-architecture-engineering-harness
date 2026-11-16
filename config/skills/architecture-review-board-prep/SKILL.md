---
name: architecture-review-board-prep
description: "Packages a proposed design into the artifacts an architecture review board (ARB) needs to make a fast, informed decision — problem statement, options considered, diagrams, risk assessment, and open questions. Use before presenting a significant design to a review board, steering committee, or cross-team architecture forum."
---

# Architecture Review Board Prep

Prepare a design so a review board can evaluate it in one sitting: a tight problem statement, the options genuinely considered (not a strawman), diagrams at the right altitude, and an explicit list of risks and open questions — instead of a rambling slide deck that forces the board to reverse-engineer what's actually being decided.

## When to use this

- A design crosses team boundaries, touches shared infrastructure, or has a large blast radius, and organizational process requires ARB sign-off before implementation starts.
- A previous ARB presentation got derailed by questions that should have been answered up front (cost, security, data ownership), burning the review slot without a decision.
- The team wants to preempt board pushback by explicitly stating trade-offs and known risks rather than presenting only the happy path and looking evasive when challenged.
- Multiple teams have competing proposals for the same problem and need a structured, comparable format so the board can evaluate them on the same axes.
- A design was already informally agreed among the immediate team but needs formal governance sign-off for compliance, budget, or cross-org visibility reasons.

## Workflow

1. **State the problem and its business/technical driver in 2-3 sentences before anything else.** The board should understand *why this decision matters now* — a new NFR, an incident, a scaling wall, a compliance deadline — before seeing any proposed solution (see `nfr-to-architecture-mapping`).
2. **Present the real options considered, not a strawman.** List 2-4 genuinely viable alternatives with their trade-offs, even ones you didn't pick — a board that suspects you only presented one real option to force a rubber stamp will push back harder, not less. Pull directly from your ADR (`architecture-decision-record-authoring`).
3. **Bring diagrams at the altitude the board actually needs**: a system context diagram (`system-context-diagram-authoring`) for scope, and a C4 container diagram (`c4-model-diagramming`) for the shape of the solution — skip component/code-level detail unless directly asked; too much detail buries the decision the board needs to make.
4. **Make the recommendation explicit and state it early**, with the reasoning that led there — don't present three neutral options and ask the board to pick; that's what the ARB is there to ratify or challenge, not invent from scratch.
5. **Pre-answer the questions every board asks**: cost impact, security/compliance implications (see `security-architecture-review`), operational ownership post-launch, rollback plan, and blast radius if it fails. Put these in the doc, not just in your head, so the meeting doesn't stall waiting for you to look something up.
6. **List known risks and open questions explicitly**, each with an owner and a target resolution date if unresolved — a board is far more comfortable approving a design with visible, owned risks than one presented as risk-free (which nobody believes).
7. **State what you're asking the board for**: full approval, conditional approval pending a named follow-up, or just feedback before you commit further. An ambiguous ask wastes the meeting on the board figuring out what decision they're being asked to make.
8. **Send the packet 24-48 hours ahead**, not at the start of the meeting — boards that read in advance ask sharper, faster questions and reach a decision in the room instead of tabling it for "review."
9. **Capture the board's decision and any conditions in writing immediately after**, and fold them back into the ADR as the final decision record — an ARB verdict that isn't written down gets re-litigated at the next review.
10. **If deferred or rejected, get the specific blocking concern in writing** before leaving the room — "come back with more detail" without specifics guarantees a second unproductive round.

## Example

Scenario: `Meridian Health`'s platform team wants ARB approval to introduce a service mesh (Istio) across 40 services, driven by a recent incident where inconsistent retry behavior between services caused a cascading outage.

Packet sent 48 hours ahead:
- **Problem**: inconsistent retry/timeout/mTLS handling across 40 services led to a cascading failure (link to postmortem); each team reimplements resiliency logic independently and inconsistently.
- **Options considered**: (a) library-based shared resiliency SDK, (b) API-gateway-level retry policy only, (c) service mesh (Istio), (d) do nothing, revisit after next incident. Trade-offs tabled for each (see `service-mesh-evaluation`).
- **Recommendation**: (c), phased rollout — 3 non-critical services first, expand after a 6-week bake period.
- **Cost**: ~0.5 additional vCPU per pod for sidecars, estimated $4,200/month at current fleet size (see `cost-aware-architecture-design`).
- **Risk register**: sidecar failure mode not yet load-tested (owner: SRE lead, resolve before phase 2); learning curve for on-call (owner: platform team, mitigated via runbook + training by phase 2).
- **Ask**: approval for phase 1 (3 services) now; phase 2 expansion requires a follow-up review with bake-period data.

Board outcome: conditional approval for phase 1 only, with a required follow-up review scheduled for phase 2 — captured in the ADR as the decision of record within the hour.

## Common pitfalls

- **Presenting a strawman set of alternatives** to steer the board toward a predetermined answer — experienced boards spot this and it costs trust for future reviews.
- **Bringing component/code-level diagrams to a scope-level conversation**, burying the actual decision the board needs to make under implementation detail.
- **Omitting cost, security, or operational-ownership answers**, forcing the meeting to pause on questions that should have been pre-answered.
- **Presenting the design as risk-free**, which reads as either naive or evasive — a design with visible, owned risks is more fundable, not less.
- **Leaving the room without a written decision or without specific blocking concerns**, guaranteeing the same ground gets re-covered next review.
- **Sending the packet at meeting start**, forcing the board to read cold and ask shallow questions instead of substantive ones.

## Output

An ARB packet (slides or a shared doc, e.g., `docs/architecture/arb/<topic>-review.md`) containing: problem statement, options considered with trade-offs, recommendation, diagrams (context + container level), cost/security/ops pre-answers, risk register with owners, and an explicit ask — plus a post-meeting decision record folded back into the governing ADR.
