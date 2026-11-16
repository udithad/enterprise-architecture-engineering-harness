---
name: legacy-integration-adapter-design
description: "Designs anti-corruption adapters that translate between a new system's clean domain model and a legacy system's constraints, protocols, and data quirks — so legacy debt doesn't leak into new code. Use when a new service or system must integrate with a legacy system it doesn't control and shouldn't be shaped by."
---

# Legacy Integration Adapter Design

Put a translation layer — an anti-corruption layer (ACL) — between a new system and a legacy system it must integrate with, so the legacy system's outdated data model, quirky protocols, and accumulated special cases stay contained at the boundary instead of leaking into and corrupting the new system's clean domain model.

## When to use this

- A new service needs to read from or write to a legacy system (mainframe, old monolith, third-party system you don't control) and the temptation is to just mirror the legacy data model directly into the new system for expediency.
- A `strangler-fig-migration-pattern` is underway and new and legacy components need to coexist and interoperate correctly during the transition.
- The legacy system's domain model has known quirks (overloaded fields, implicit business rules encoded in magic values, inconsistent null handling) that would corrupt the new system's model if adopted wholesale.
- Multiple new services are each independently building their own ad hoc translation logic against the same legacy system, duplicating (and inconsistently interpreting) the same quirks.
- The legacy system has limited or fragile integration surfaces (a brittle SOAP API, a flat-file export, direct DB access) that need to be isolated behind a stable contract so the new system's design isn't dictated by them.

## Workflow

1. **Model the new system's domain cleanly first, independent of the legacy system's shape** — define the target domain model based on what the new system actually needs, not what the legacy system happens to expose. This ordering matters: designing the new model *from* the legacy schema imports its distortions by construction.
2. **Identify every legacy quirk that needs translation**: overloaded/multi-purpose fields, magic values encoding implicit state (`status = 9` meaning "cancelled by mistake, do not process"), inconsistent units or formats, and business rules that exist only as tribal knowledge about how the legacy system behaves, not in any schema.
3. **Design the adapter as the single translation point** — one component (or a small, well-defined set) responsible for converting legacy representations into the new domain model on the way in, and back on the way out if bidirectional. No other part of the new system should ever see raw legacy data structures directly.
4. **Isolate legacy protocol/transport concerns inside the adapter too**, not just data shape — if the legacy system only speaks SOAP, fixed-width files, or requires polling a shared database table, that mechanism lives entirely inside the adapter; the new system's domain and application layers interact only with a clean, protocol-agnostic interface (ports/adapters — see `hexagonal-architecture-design`).
5. **Handle legacy data quality defensively, explicitly, and visibly** — validate incoming legacy data against the new domain model's invariants, and decide deliberately what happens on violation: reject with a clear error, quarantine for manual review, or apply a documented default — never silently accept malformed data into the clean model.
6. **Version the adapter's contract independently of the legacy system's own (lack of) versioning.** Legacy systems often can't be versioned safely; the adapter absorbs that instability by presenting a stable, versioned interface to the new system even while what's behind it might change or need workarounds.
7. **Log and monitor translation failures and quirk-encounters as first-class signals** — a spike in "unrecognized legacy status code" errors is valuable data about the legacy system's actual (as opposed to documented) behavior, and often surfaces business rules nobody remembered.
8. **Avoid building the adapter to be "smart"** — it translates and validates; it should not accumulate new business logic of its own. An adapter that grows business rules over time becomes a second, harder-to-reason-about domain model competing with the real one.
9. **Plan the adapter's retirement from day one if this integration is transitional** (part of a strangler-fig migration) — track what fraction of traffic still needs the legacy path, and design the adapter to be deleted cleanly once the legacy system is fully retired, rather than becoming permanent hidden infrastructure nobody remembers the purpose of.
10. **Test the adapter heavily against real legacy data samples**, not just synthetic happy-path data — legacy systems accumulate decades of edge cases that only show up in production data, and the adapter's value is precisely in handling those correctly and predictably.

## Example

Scenario: `Meridian Bank`'s new loan-origination service needs to read customer account data from a 25-year-old mainframe system. The mainframe's `ACCOUNT` record uses a single `STATUS` field with 14 different numeric codes (some meaning genuinely different things, some legacy duplicates from a system merge a decade ago), dates stored as `YYYYMMDD` integers (with `00000000` meaning "no date" and `99999999` meaning "indefinite"), and a `FLAGS` bitfield where bit 3 means "frozen" but only if bit 7 is also unset (an undocumented interaction discovered by reading old COBOL).

Adapter design:
- **Domain model (new system)**: `Account { status: AccountStatus (enum: ACTIVE, CLOSED, FROZEN, PENDING_REVIEW), openedDate: Optional<LocalDate>, ... }` — clean, unambiguous, with no trace of the mainframe's encoding.
- **Translation table**: all 14 legacy status codes mapped explicitly to the 4-value `AccountStatus` enum, with the mapping table itself code-reviewed and stored as a versioned config, including comments explaining *why* codes 6 and 11 (both legacy "closed" variants from the historical merge) map to the same enum value.
- **Date handling**: `00000000` → `Optional.empty()`, `99999999` → a documented sentinel handled explicitly by callers who care about "indefinite," never silently converted to `null` and mishandled downstream.
- **Bitfield logic**: the bit-3-unless-bit-7 interaction is encoded once, in the adapter, with a comment referencing the COBOL source it was reverse-engineered from — no other part of the new system ever touches `FLAGS` directly.
- **Failure handling**: an unrecognized status code (a 15th value appearing for the first time) is logged as a `LegacyTranslationAnomaly` metric and the account is quarantined for manual review rather than silently defaulted to `ACTIVE`.
- **Protocol isolation**: the mainframe is only reachable via a nightly batch file export; the adapter's ingestion component handles file parsing and polling entirely internally, exposing only a clean `AccountRepository` port to the rest of the loan-origination service.

Result: the loan-origination domain model never has to know the mainframe exists; when the mainframe is eventually decommissioned as part of a longer migration, only the adapter needs to change (or be deleted), with zero impact on the domain and application layers built against it.

## Common pitfalls

- **Mirroring the legacy data model directly into the new system "to save time,"** importing decades of legacy quirks into what was meant to be a clean design.
- **Letting legacy data structures leak past the adapter** into application or domain code anywhere in the new system, defeating the whole purpose of the isolation.
- **Silently defaulting on unrecognized or malformed legacy data** instead of surfacing it — this hides real business-rule gaps until they cause a downstream incident.
- **Letting the adapter accumulate business logic over time**, turning it into an unofficial second domain model that's harder to reason about than either system alone.
- **Multiple teams building separate, inconsistent ad hoc translation logic** against the same legacy system instead of sharing one adapter, leading to divergent interpretations of the same quirks.
- **Never planning for the adapter's retirement**, so it outlives the migration it was built for and becomes permanent, unexplained infrastructure.
- **Testing only against synthetic happy-path data**, missing the real-world edge cases that are exactly what legacy systems accumulate over decades.

## Output

An anti-corruption adapter (a bounded, testable component, e.g., `adapters/legacy-mainframe/`) with an explicit translation table/mapping for every known legacy quirk, a documented failure-handling policy for unrecognized data, protocol isolation from the new system's domain and application layers, and — where the integration is transitional — a tracked retirement plan tied to the broader migration.
