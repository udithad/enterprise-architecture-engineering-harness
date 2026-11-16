---
name: technology-radar-evaluation
description: "Runs an Adopt/Trial/Assess/Hold technology radar process to evaluate new tools, languages, frameworks, or platforms systematically, before a team commits to them. Use when a new technology is proposed, when uncoordinated tool sprawl is emerging across teams, or when running a periodic technology review."
---

# Technology Radar Evaluation

Evaluate a new tool, language, framework, or platform through a structured Adopt/Trial/Assess/Hold process — with explicit evaluation criteria, a defined trial scope, and periodic review — instead of technology choices happening ad hoc per team, driven by whoever's most enthusiastic about the newest thing.

## When to use this

- A team wants to introduce a new language, framework, database, or platform, and the organization needs a consistent way to evaluate it rather than a one-off pitch.
- Multiple teams have independently adopted different tools for the same job (three different message queues, two different ORMs) and uncoordinated sprawl is creating maintenance and hiring cost.
- Running a periodic (quarterly/biannual) technology review to keep the organization's tech stack intentional rather than accreting by accident.
- A `build-vs-buy-decision-framework` evaluation has concluded "build," and the specific technology choice for the build now needs its own evaluation.
- Preparing guidance for new projects on which technologies are safe defaults, which are still experimental, and which are actively discouraged.

## Workflow

1. **Define the four rings clearly and use them consistently**: **Adopt** — proven, recommended as the default choice for new work in this category. **Trial** — worth using on a real project by a team willing to accept some risk, not yet a default. **Assess** — worth understanding and experimenting with (a spike, a prototype), not yet used in production. **Hold** — proceed with caution; either being phased out, or not recommended for new adoption even if in use somewhere.
2. **Organize entries by quadrant** (a common radar convention): Languages & Frameworks, Tools, Platforms & Infrastructure, Techniques & Practices — so the radar covers more than just "which framework," including practices like a testing approach or a deployment technique.
3. **Define explicit evaluation criteria before scoring anything**: maturity and community/vendor support, fit with existing skills and stack, operational burden, licensing/cost, security track record, and migration/exit cost if adopted and later reversed (`vendor-lock-in-risk-assessment` overlaps here for vendor-specific technology).
4. **Require evidence for ring placement, not opinion alone** — a "Trial" placement should point to an actual pilot project's outcome or a documented spike; an "Adopt" placement should point to production usage and its results, not just "we've heard good things."
5. **Scope trials deliberately and time-box them** — a technology moving from Assess to Trial should have a named pilot project, a defined success criteria, and a review date, not an open-ended "someone's using it somewhere."
6. **Solicit input broadly but keep a small group accountable for the final placement** — a radar written by consensus of everyone tends toward mush; a radar written by one architect in isolation misses real field experience. A working group that gathers input but makes the call works better than either extreme.
7. **Publish the radar with rationale per entry**, not just a ring placement — "why is GraphQL in Hold" needs a stated reason (e.g., "two prior adoptions had operational issues with caching and N+1 query patterns; revisit after tooling matures") so the placement is a decision, not a verdict from nowhere.
8. **Review and republish on a fixed cadence** (quarterly or biannual) — technology and the organization's own experience with it both move; a radar that's never updated becomes stale guidance that teams learn to ignore.
9. **Connect radar placements to actual governance** — if something is in Hold, is there a real process for a team that still needs an exception, or does Hold mean "no exceptions"? An unenforced radar becomes decorative.
10. **Track technologies moving between rings over time** as a signal in itself — something repeatedly stuck in Assess without ever trialing suggests either nobody has bandwidth to pilot it or it's not actually valuable enough to prioritize; make that visible rather than leaving it ambiguous.

## Example

Scenario: `Meridian Bank`'s platform engineering group runs a quarterly technology radar to keep ~40 engineering teams from independently reinventing infrastructure choices.

Sample entries from the Q3 radar:

| Technology | Quadrant | Ring | Rationale |
|---|---|---|---|
| PostgreSQL | Languages & Frameworks... (Platforms) | **Adopt** | Default relational store for all new services; strong internal ops experience, proven at current scale in 30+ services. |
| gRPC | Languages & Frameworks | **Trial** | Piloted successfully in the payments team's service-to-service calls (see `grpc-vs-rest-vs-graphql-decision`); recommend for new internal service-to-service APIs where a team is comfortable with the tooling, not yet a default for all cases. |
| Kafka | Platforms & Infrastructure | **Adopt** | In production for 3 years across 12 teams; standard choice for `async-messaging-architecture` needs. |
| A new vector database (evaluated this quarter) | Platforms & Infrastructure | **Assess** | One team is running a spike for a search-relevance use case; not yet validated at production scale or against the bank's data-residency requirements. Revisit Q1 next year with spike results. |
| Team-specific custom ORM (found in two teams) | Languages & Frameworks | **Hold** | Two teams independently built custom ORM wrappers instead of using the org-standard library; increases onboarding cost and diverges from the Adopt-ring choice with no clear benefit found. Teams using it are asked to migrate at next major version bump, not immediately, given cost. |
| GraphQL for public APIs | Languages & Frameworks | **Hold** | Prior adoption for the partner API surfaced caching and N+1 query operational issues without a clear owner for the added complexity; hold until a team proposes a concrete plan for query cost limiting and caching strategy. |

Each entry links to a one-paragraph rationale doc and, where applicable, the pilot project that produced the evidence. The radar is reviewed by a 5-person working group (rotating membership) that solicits nominations from all 40 teams but makes the final ring call, and republished each quarter with a visible diff from the prior version.

## Common pitfalls

- **Ring placement based on opinion or hype rather than evidence** — an "Adopt" with no production track record behind it is just an enthusiastic guess.
- **No time-boxing or success criteria for Trial-ring pilots**, so a trial runs indefinitely without ever producing a decision to promote or roll back.
- **A radar written by full consensus**, producing vague, uncontroversial placements that don't actually guide decisions.
- **No rationale published per entry**, leaving teams unable to understand or challenge a placement, and treating the radar as an oracle rather than a reasoned document.
- **Never revisiting the radar**, letting it go stale while the organization's actual technology landscape moves on.
- **No connection between radar placement and real governance** — a "Hold" that nobody enforces or that has no exception process becomes ignored.
- **Radar entries too narrow** (only frameworks), missing tooling, platform, and practice choices that are equally worth evaluating consistently.

## Output

A published technology radar (a visual radar chart plus a table) with each technology in one quadrant and one ring, a stated rationale and evidence source per entry, a defined review cadence, and a governance note on how Hold/Adopt placements connect to actual team decisions and exception processes.
