---
name: build-vs-buy-decision-framework
description: "Scores build-vs-buy options against total cost, control, time-to-market, and differentiation value, producing a structured, defensible recommendation instead of a gut call. Use when a team is deciding whether to build a capability in-house (auth, search, payments, notifications) or adopt a vendor/OSS solution."
---

# Build vs. Buy Decision Framework

Make the build-vs-buy call on explicit, weighted criteria — cost over the actual horizon, strategic differentiation, control/customization needs, and time-to-market — instead of defaulting to "we'll build it" out of engineering pride or "we'll buy it" out of a rushed deadline.

## When to use this

- A team is debating whether to build a capability (auth, search, feature flags, notifications, billing) in-house or adopt a vendor/OSS/SaaS solution, and the decision keeps getting made informally in a Slack thread.
- Leadership wants a defensible, written rationale for a build-vs-buy call that will be scrutinized later (by finance, by a new tech lead, by an auditor).
- A prior "build" decision is being revisited because the in-house solution has become an ongoing maintenance burden that wasn't accounted for at the time.
- Multiple teams are independently building similar internal tools (three teams each built their own retry/queue wrapper) and a platform-level buy-vs-standardize decision is needed.
- A vendor evaluation (`technology-radar-evaluation`) has shortlisted options and the team needs to weigh them against a genuine build option, not just against each other.

## Workflow

1. **Separate the capability into "core differentiator" vs. "undifferentiated plumbing."** If the capability is something customers pay you for and no vendor does it as well as your domain expertise (a proprietary matching algorithm, your core product logic), lean build. If it's something every company needs and doesn't differentiate you (SSO, email delivery, PDF generation), lean buy — you're unlikely to build it better than a vendor who does nothing else.
2. **Estimate build cost over the full horizon, not just initial build.** Include design, implementation, testing, security hardening, ongoing maintenance, on-call burden, and the opportunity cost of the engineers who build it not working on the differentiating product — a common underestimate is treating "build cost" as only the first release.
3. **Estimate buy cost over the same horizon**: license/subscription fees at projected scale (not today's scale — check pricing tiers), integration effort, migration cost if you ever need to leave (see `vendor-lock-in-risk-assessment`), and the cost of working around any gaps between the vendor's feature set and your actual requirements.
4. **Score both options against explicit weighted criteria**: total cost of ownership, time-to-market, control/customization ceiling, security/compliance fit, vendor stability/lock-in risk, and team's existing expertise. Weight the criteria before scoring options, not after, to avoid rationalizing a pre-decided answer.
5. **Assess the "good enough" threshold honestly.** A vendor solution covering 90% of requirements with a workable path for the remaining 10% often beats a custom build that covers 100% but takes 6 extra months and an ongoing maintenance team — unless that 10% is the differentiating part.
6. **Factor in switching cost and reversibility.** A buy decision with a clean data-export path and no proprietary lock-in is a much lower-risk bet than one requiring deep, hard-to-unwind integration — score reversibility explicitly, don't treat all "buy" options as equally risky.
7. **Include the hidden organizational cost of build**: an in-house solution needs an owning team forever, survives team reorgs and attrition, and becomes someone's unglamorous on-call burden — name who owns it long-term before committing to build.
8. **Pilot before committing at scale** where possible — a paid trial or a scoped proof-of-concept for "buy," or a time-boxed spike for "build" — to replace assumptions with real data before the full decision.
9. **Write the decision as an ADR** (`architecture-decision-record-authoring`) capturing the scored criteria, the chosen option, and — critically — the conditions under which this decision should be revisited (e.g., "revisit if vendor pricing changes" or "revisit if this becomes a core differentiator").
10. **Set a revisit trigger, not just a revisit date.** Build-vs-buy is rarely permanent; state the specific signal (10x scale, a pricing change, a new compliance requirement, the vendor being acquired) that should trigger re-evaluation rather than leaving it to be reconsidered only when something breaks.

## Example

Scenario: `NimbusCRM` needs a notification system (email, SMS, push) for customer-facing alerts. The team is split: some want to build on top of existing SMTP/SMS provider APIs directly; others want to adopt a third-party notification platform (e.g., a hosted multi-channel notification service).

Scoring (weights out of 10):

| Criterion (weight) | Build | Buy |
|---|---|---|
| Time-to-market (25%) | 8 weeks, score 4 | 2 weeks, score 9 |
| TCO over 3 years (25%) | ~$180K eng time, score 6 | ~$90K subscription + integration, score 8 |
| Differentiation (20%) | Not a differentiator, score 3 (low value in building) | score 8 |
| Control/customization (15%) | Full control, score 9 | Template/rate-limit constraints, score 6 |
| Lock-in/reversibility (15%) | score 9 (fully owned) | score 6 (has export API, moderate switching cost) |

Weighted totals: Build ≈ 5.9, Buy ≈ 7.5. Recommendation: buy, with a named revisit trigger — "revisit if per-message cost at 10x current volume exceeds in-house TCO" (calculated threshold: >4M messages/month). Decision and threshold recorded in an ADR; owning team for the vendor relationship (not a build team) is named.

## Common pitfalls

- **Comparing today's build cost against the vendor's full multi-year cost**, or vice versa — both estimates need to cover the same horizon to be comparable.
- **Treating "buy" as zero-maintenance** — integration code, contract renewals, and working around vendor gaps are ongoing costs that get left out of the estimate.
- **Building undifferentiated plumbing out of engineering pride**, tying up scarce engineering time on something a vendor already does well.
- **Buying a core differentiator** because it was faster, then discovering the vendor can't support the customization that actually matters to customers.
- **No named long-term owner for a "build" decision**, so the in-house solution becomes an orphaned system nobody wants to maintain after the original team moves on.
- **No revisit trigger**, leaving a decision made under yesterday's constraints (scale, pricing, requirements) unexamined for years.

## Output

A build-vs-buy decision doc or ADR (e.g., `docs/architecture/build-vs-buy-<capability>.md`) with the weighted scoring table, the full-horizon cost estimates for both paths, the chosen option and its named long-term owner, and an explicit revisit trigger.
