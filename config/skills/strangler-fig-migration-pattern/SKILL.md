---
name: strangler-fig-migration-pattern
description: "Plans an incremental legacy-system replacement where a façade routes traffic between old and new implementations feature by feature, until the legacy system can be safely retired. Use when a legacy system needs replacing but a big-bang rewrite is too risky, too slow, or would freeze feature delivery for too long."
---

# Strangler Fig Migration Pattern

Replace a legacy system incrementally by placing a façade in front of it that routes each piece of functionality to either the legacy system or its new replacement, gradually moving more traffic to the new system feature by feature until the legacy system handles nothing and can be safely retired — avoiding the high risk of a big-bang rewrite.

## When to use this

- A legacy system needs replacing, but a full rewrite-and-cutover would take too long, freeze feature delivery, or carry too much risk to do as one atomic switch.
- `monolith-to-microservices-migration-plan` has identified specific bounded contexts to extract, and needs a concrete mechanism for routing traffic during the transition.
- A previous big-bang rewrite attempt failed or stalled (a common outcome — the new system tries to match 100% of the old system's behavior before ever shipping, and momentum dies).
- The legacy system must keep operating and serving production traffic throughout the migration — there's no acceptable maintenance window for a full cutover.
- Business stakeholders need to see incremental value and reduced risk exposure throughout a multi-quarter or multi-year migration, not a single high-stakes cutover date.

## Workflow

1. **Identify the seams to strangle first** — decompose the legacy system's functionality into independently-migratable pieces (by bounded context, by API endpoint, by user segment), using the same boundary analysis as `microservices-decomposition` or `domain-driven-design-context-mapping`. Pick pieces with clear boundaries and low coupling to migrate first.
2. **Build the façade/routing layer** in front of the legacy system — every request now passes through it, even before any migration happens, so the routing mechanism itself is proven safe before it starts making real decisions.
3. **Migrate the first, lowest-risk piece to the new implementation**, keeping the legacy system as the fallback. Route only that piece's traffic to the new implementation via the façade; everything else still goes to legacy.
4. **Run new and legacy in parallel with comparison where correctness is high-stakes** — for critical logic, consider a "shadow mode" where the new implementation runs alongside the legacy one on live traffic, results are compared, and only the legacy result is returned to the user until confidence is established (a form of `canary-release-validation-testing` applied to a rewrite).
5. **Cut traffic over gradually per piece** (percentage-based rollout, or by user segment) rather than all-or-nothing, so a regression in the new implementation affects a bounded, controlled subset of traffic and can be rolled back via the façade without touching legacy.
6. **Keep the legacy system's data as the system of record until the new system is proven**, or design explicit data synchronization if both systems need to read/write during the transition — an unplanned dual-write scenario is a common source of silent data divergence during strangler migrations.
7. **Track migration progress as a measurable ratio** (percentage of traffic, or number of endpoints/features) moved to the new system, visible to stakeholders, so the migration's progress is concrete rather than "in progress" indefinitely.
8. **Decommission legacy code paths as soon as a piece is fully migrated and stable**, not "eventually" — leaving dead legacy code running unused is wasted operational risk and cost; each piece's migration isn't complete until the legacy path is actually removed.
9. **Watch for the façade itself becoming a long-term architectural fixture** rather than a temporary migration tool — plan its own removal or simplification once the legacy system is fully retired, so the system isn't left with permanent unnecessary routing indirection.
10. **Revisit prioritization as the migration progresses** — the initially-planned migration order may need to change based on what's actually learned (a "simple" piece turns out to have hidden legacy coupling; another piece becomes higher business priority).

## Example

Scenario: `InvoiceCore`, a 15-year-old monolithic billing system, needs replacing. A prior big-bang rewrite attempt stalled after 18 months without shipping, because the new system tried to replicate every legacy edge case before launch.

Strangler fig plan:
- **Façade**: an API gateway placed in front of `InvoiceCore`, initially routing 100% of traffic straight through unchanged — proving the routing layer adds no regressions before any real migration begins.
- **First piece migrated**: invoice PDF generation — chosen because it's read-only, has a clear input/output contract, and a bug here is visibly wrong (a malformed PDF) rather than silently wrong (an incorrect charge), making it low-risk to validate.
- **Shadow mode**: for two weeks, the new PDF generator runs on every real invoice request alongside the legacy one; outputs are diffed automatically, and only the legacy PDF is served to users. Divergences are triaged daily.
- **Gradual cutover**: once shadow-mode diff rate drops below 0.1% (remaining diffs are legacy formatting quirks judged acceptable to drop), the façade begins serving the new generator's output to 5% of traffic, then 25%, 100%, over three weeks, with rollback-to-legacy available at each stage via a feature flag.
- **Decommission**: once at 100% for two stable weeks, the legacy PDF generation code path is deleted, not just disabled — reducing the legacy system's surface area for the next piece.
- **Next piece**: invoice calculation logic (write-path, higher stakes) — deliberately sequenced after the lower-risk PDF piece, applying lessons learned (the shadow-mode diffing tooling built for PDFs is reused and extended for calculation comparison).
- **Progress tracking**: a dashboard shows "3 of 14 identified legacy modules migrated, representing 22% of total legacy request volume," reviewed monthly with stakeholders.

Result: after 8 months, five of the highest-value, lowest-risk modules are fully migrated and legacy code for them deleted, with continuous production stability throughout — a contrast to the stalled big-bang attempt, and with visible incremental progress stakeholders can track.

## Common pitfalls

- **Attempting to migrate everything at once "since we're doing a rewrite anyway"**, recreating the big-bang risk the pattern is meant to avoid.
- **Never decommissioning legacy code paths after a piece is migrated**, leaving the system running two implementations of the same functionality indefinitely, doubling maintenance burden.
- **No shadow-mode or comparison step for high-stakes logic**, cutting over financial or otherwise critical calculations without validating the new implementation against real traffic first.
- **Unplanned dual-write between legacy and new systems** during the transition, causing silent data divergence that's discovered much later.
- **Treating the façade as a permanent architectural layer** rather than a temporary migration tool, leaving unnecessary routing indirection after legacy is fully retired.
- **Migrating pieces in an order driven by convenience rather than risk/value**, spending early migration effort on low-value pieces while the highest-risk legacy code remains untouched longest.
- **No visible progress metric**, so the migration appears perpetually "in progress" with no way for stakeholders to see real advancement.

## Output

A strangler fig migration plan: the identified seams/pieces in migration order with rationale, the façade/routing design, the shadow-mode/comparison strategy for high-stakes pieces, the gradual cutover mechanism (percentage or segment-based) with rollback path, a tracked progress metric, and an explicit decommissioning step per piece including eventual façade removal.
