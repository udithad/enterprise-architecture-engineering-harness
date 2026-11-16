---
name: architecture-fitness-function-design
description: "Encodes architectural rules — layering, coupling limits, latency budgets, dependency direction — as automated tests that run in CI, so architecture erosion is caught the moment it happens instead of discovered months later in a review. Use when a team has documented architectural rules that keep getting silently violated, or when preparing to protect a migration's target shape."
---

# Architecture Fitness Function Design

Turn architectural intent — "the domain layer must not depend on the web layer," "service A must never call service B's database directly," "this endpoint must respond under 200ms at p95" — into automated checks that run on every commit, so architecture is enforced continuously instead of relying on humans remembering the rules during code review.

## When to use this

- An ADR or design doc states a rule (layering, dependency direction, no circular imports, a latency budget) but nothing actually stops someone from violating it next sprint.
- A past architecture review found the same violation being reintroduced repeatedly because nothing catches it until the next manual review, months later.
- A `strangler-fig-migration-pattern` or `monolith-to-microservices-migration-plan` is underway and the team needs to make sure already-migrated code doesn't quietly get coupled back to the legacy system.
- Onboarding new engineers keeps producing accidental violations (a new service calling another service's database, a domain class importing a framework type) because the rule lives only in a wiki page nobody reads.
- Leadership wants objective, trend-able evidence that architecture quality is improving or degrading over time, not just anecdote at the next review.

## Workflow

1. **Extract testable rules from existing architecture docs and ADRs.** Read `architecture-decision-record-authoring` outputs, C4 diagrams, and layering docs; pull out any rule phrased as a constraint ("X must not depend on Y", "no more than N inbound dependencies on this module", "P99 latency under 300ms") — vague aspirations ("keep it clean") aren't fitness-function material until made specific.
2. **Classify each rule by fitness-function type**: structural (dependency direction, layering, module boundaries), latency/throughput, resiliency (chaos/failure injection), security (no plaintext secrets, forbidden imports), or process (deployment frequency, MTTR). Different types need different tooling.
3. **Pick the enforcement mechanism per rule type.** Structural rules: ArchUnit (Java), import-linter or `deptry` (Python), `depcruise`/`eslint` custom rules, or a small static-analysis script for C++ include graphs. Latency rules: assertions in load-test pipelines. Security rules: SAST/lint rules. Prefer tools that already run in the team's stack over bespoke scripts.
4. **Write each fitness function as a normal automated test**, named and grouped so failures are self-explanatory (e.g., `ArchitectureTest.domainLayerMustNotDependOnWebLayer`), with a failure message that states the rule and points at the violating file — not just "test failed."
5. **Decide severity per rule: blocking vs. warning.** New, strict rules that would fail on existing code should start as warnings (or run against a baseline/allowlist) to avoid halting the whole team on day one; tighten to blocking once the codebase is clean and the team has bought in.
6. **Wire fitness functions into CI as a required (or advisory) check**, running on every PR, not as a separate nightly job that gets ignored — the whole point is fast feedback before merge.
7. **Baseline known violations explicitly** rather than disabling the rule. An allowlist file listing current exceptions (with a ticket reference and target removal date) lets you turn a rule on today without an all-or-nothing rewrite, while still blocking *new* violations.
8. **Track fitness-function results over time as an architecture health signal** — trend the count of allowlisted exceptions, latency-budget violations, or coupling-metric drift on a dashboard leadership and the team both see, feeding `technical-debt-tracking` conversations with data instead of opinion.
9. **Review and retire fitness functions periodically.** Rules tied to a since-completed migration (e.g., "no calls from new services to the legacy monolith's DB") should be removed once the legacy system is gone — stale fitness functions that no longer reflect intent just add noise and slow builds.
10. **Socialize failures as teaching moments, not gatekeeping.** When a fitness function fails a PR, the failure message and a linked doc should explain *why* the rule exists, so it reinforces the architecture instead of feeling like arbitrary friction.

## Example

Scenario: `LedgerCore`, a Java-based accounting platform, adopted a hexagonal architecture (see `hexagonal-architecture-design`) two years ago. A recent architecture review found the domain layer had accumulated three direct Spring `@Autowired` framework imports and one JDBC call bypassing the repository port — violations nobody caught because they were small, individually reasonable-looking changes.

Fitness functions added:

```java
@ArchTest
static final ArchRule domain_must_not_depend_on_spring =
  noClasses().that().resideInAPackage("..domain..")
    .should().dependOnClassesThat().resideInAPackage("org.springframework..");

@ArchTest
static final ArchRule domain_must_not_use_jdbc_directly =
  noClasses().that().resideInAPackage("..domain..")
    .should().accessClassesThat().resideInAPackage("java.sql..");
```

Rollout: both rules initially run as `warning` severity against the current codebase, with the 4 known violations listed in `arch-fitness-allowlist.yml` alongside a ticket to fix each. After the violations are fixed (3 sprints later), both rules flip to `blocking` in CI. A third fitness function is added for the in-flight migration: `noClasses().that().resideInAPackage("..newservices..").should().dependOnClassesThat().resideInAPackage("..legacyMonolith..")`, protecting the strangler-fig boundary until the monolith is fully retired, at which point the rule is deleted.

## Common pitfalls

- **Writing rules too vague to automate** ("keep coupling low") instead of a specific, checkable threshold or dependency direction.
- **Turning every new rule on as blocking immediately**, failing the whole team's builds on pre-existing violations and burning goodwill for the practice.
- **Letting the allowlist grow forever** because exceptions are added but never revisited — an allowlist without an expiry or owning ticket becomes a permanent escape hatch.
- **No link between a failing fitness function and the reasoning behind the rule**, so engineers experience it as arbitrary CI friction rather than architecture guidance.
- **Fitness functions that duplicate what a linter or SAST tool already does**, adding maintenance burden for no new coverage.
- **Never retiring rules tied to a completed migration**, leaving stale checks that slow CI and confuse new engineers about what's actually still in flight.

## Output

A `fitness-functions/` suite (co-located with the relevant module, e.g., `arch-test/`) mapping each architectural rule to an automated test, an `arch-fitness-allowlist.yml` for tracked exceptions with owners and target dates, and a CI job that runs the suite on every PR — plus a short doc explaining each rule's rationale for engineers who hit a failure.
