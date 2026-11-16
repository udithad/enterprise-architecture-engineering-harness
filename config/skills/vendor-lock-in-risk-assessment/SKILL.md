---
name: vendor-lock-in-risk-assessment
description: "Quantifies switching costs and defines an exit strategy before committing to a vendor, cloud-specific service, or proprietary technology, so lock-in is a knowingly accepted trade-off rather than a surprise discovered at renewal time. Use before adopting a vendor-specific service, and periodically for existing critical dependencies."
---

# Vendor Lock-In Risk Assessment

Quantify what it would actually cost to leave a vendor or proprietary technology — in engineering time, data migration effort, and business disruption — before committing to it, so lock-in becomes a knowingly accepted trade-off with a defined exit plan, rather than a constraint discovered at contract renewal when negotiating leverage is already gone.

## When to use this

- A team is proposing to adopt a vendor-specific cloud service (a proprietary managed database, a cloud-specific serverless framework, a vendor's proprietary API) and the lock-in trade-off hasn't been made explicit.
- A `build-vs-buy-decision-framework` evaluation is leaning toward "buy" and needs the lock-in dimension quantified as part of the decision, not just cost and time-to-market.
- A vendor renewal negotiation is approaching and the team needs to know its actual switching cost to negotiate from an informed position instead of a position of assumed helplessness.
- A vendor has shown signs of risk (pricing changes, acquisition, service deprecations, declining support quality) and the organization needs to assess exposure before deciding whether to stay or plan an exit.
- Reviewing a system's dependency list periodically to catch lock-in that accumulated gradually without any single decision point.

## Workflow

1. **Distinguish switching cost from lock-in risk** — every vendor choice has some switching cost; the question is whether that cost is proportionate to the value gained, and whether it's been sized, not whether it's zero. A hard "avoid all lock-in" stance often means giving up genuinely valuable managed services for no real benefit.
2. **Classify the type of lock-in**: data lock-in (proprietary format, hard to export), API/interface lock-in (code written directly against a vendor-specific API with no abstraction), operational lock-in (team skills and tooling built entirely around one vendor's console/CLI), and contractual/pricing lock-in (volume discounts or commitments that penalize leaving).
3. **Quantify the switching cost concretely**: estimate engineer-weeks to migrate data out (including any format conversion), engineer-weeks to rewrite integration code, expected downtime or risk during migration, and any contractual exit penalty. A number, even a rough one, is far more useful than "it would be hard."
4. **Assess likelihood alongside cost** — a high switching cost is a manageable risk if the vendor is stable, well-governed, and pricing is predictable; the same switching cost is a serious risk if the vendor is a small startup, has a history of pricing changes, or the relationship is strategically adversarial (a competitor's platform).
5. **Weigh lock-in against the alternative's real cost** — building and maintaining a portable, vendor-agnostic abstraction layer has its own ongoing engineering cost; sometimes accepting lock-in is the correct trade-off because the abstraction cost would exceed the realistic switching cost, especially for a service unlikely to actually need replacing.
6. **Design an abstraction seam deliberately where it's cheap and valuable** — an interface/port (see `hexagonal-architecture-design`) around a vendor-specific SDK costs little upfront and meaningfully reduces future switching cost for the code layer, even if the underlying data format still needs a migration plan.
7. **Negotiate data portability and export capability up front**, before signing a contract, not after — confirm the vendor supports exporting data in a usable, documented format, and treat "no reasonable export path" itself as a red flag independent of price.
8. **Document the accepted risk explicitly as an ADR** when choosing to accept meaningful lock-in — state the switching cost estimate, the likelihood assessment, and why the trade-off was accepted, so it's a recorded decision rather than an unexamined default.
9. **Track lock-in exposure as a portfolio, not per-decision** — a system might have many small individually-reasonable vendor dependencies that together create broad lock-in; periodically total the exposure across the whole system, not just at each individual adoption decision.
10. **Revisit assessments on a schedule tied to contract renewals** — lock-in risk isn't static; a vendor's pricing, stability, or roadmap can change, and the assessment should be refreshed before each renewal decision rather than only at initial adoption.

## Example

Scenario: `StreamWorks`, a media analytics platform, is deciding whether to build its event-ingestion pipeline on a specific cloud provider's proprietary streaming service versus a self-managed Kafka cluster.

Assessment:
- **Lock-in type**: primarily API/interface lock-in (the proprietary service's SDK and semantics differ meaningfully from open alternatives) plus some data lock-in (a proprietary internal storage format for retained stream data).
- **Switching cost estimate**: rewriting the ~15 producer/consumer integration points against a new API estimated at 6 engineer-weeks; migrating 90 days of retained stream data out via the vendor's export API estimated at 2 engineer-weeks plus a bounded cost per GB egress; total estimated at ~8 engineer-weeks and a one-time data egress fee, with roughly 3 days of reduced-redundancy operation during cutover.
- **Likelihood/vendor risk**: the vendor is the platform's existing primary cloud provider (already a broader dependency), has had stable pricing for this specific service over 4 years, and offers a documented export API — assessed as low-to-moderate risk.
- **Trade-off decision**: accept the lock-in. The proprietary service saves an estimated 0.75 FTE of ongoing operational burden versus self-managed Kafka (based on the team's prior Kafka-ops experience at a previous employer), and the quantified switching cost (~8 engineer-weeks, one-time) is judged proportionate and recoverable if ever needed — a self-managed Kafka cluster's ongoing operational cost across several years would exceed the one-time switching cost being avoided.
- **Mitigation designed anyway**: producer/consumer code is written against a thin internal interface (`EventPublisher`, `EventConsumer`) rather than calling the vendor SDK directly throughout the codebase — this doesn't reduce the data-migration cost but meaningfully reduces the code-rewrite portion of the switching cost estimate for the future, at low upfront cost.
- **Documented**: recorded as an ADR stating the accepted lock-in, the switching cost estimate, and the mitigation, with a note to re-assess at the vendor contract's 3-year renewal point.

## Common pitfalls

- **Treating "avoid lock-in" as an absolute rule**, giving up genuinely valuable managed services for portability that's never actually exercised.
- **Never quantifying switching cost**, leaving "it would be hard to leave" as an unexamined assumption instead of an estimated number that can inform the decision.
- **Confusing switching cost with lock-in risk** — a high switching cost with a stable, low-risk vendor is very different from the same cost with an unstable one, but often treated identically.
- **Writing vendor SDK calls directly throughout the codebase** with no abstraction seam, when a cheap interface layer would have meaningfully reduced future code-rewrite cost.
- **No data export/portability check before signing a contract**, discovering only at exit time that the vendor has no reasonable export path.
- **Assessing lock-in only at initial adoption**, never revisiting it as the vendor relationship or the organization's dependency portfolio evolves.
- **Ignoring the accumulated portfolio effect** of many individually-small, reasonable vendor dependencies that together create serious aggregate lock-in.

## Output

A vendor lock-in risk assessment (or ADR) stating: the type(s) of lock-in, a concrete switching-cost estimate (engineer-time, migration risk, contractual penalty), a vendor stability/likelihood assessment, the trade-off decision and rationale, any abstraction/mitigation designed to reduce future switching cost, and a scheduled re-assessment date tied to contract renewal.
