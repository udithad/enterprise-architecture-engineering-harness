---
name: domain-driven-design-context-mapping
description: "Maps bounded contexts and the relationships between them (upstream/downstream, shared kernel, anti-corruption layer, conformist) so teams agree on where one model's meaning ends and another's begins. Use before drawing service boundaries, when the same term means different things in different parts of the org, or when integration between two teams keeps breaking because their models silently diverge."
---

# Domain-Driven Design Context Mapping

Find the places where the ubiquitous language changes meaning across the organization — where "Customer" in Sales means a lead with a phone number and "Customer" in Billing means an entity with a payment method and tax ID — and make the boundary and the relationship between those two models explicit instead of pretending it's one shared concept.

## When to use this

- Two teams keep having integration bugs because they each assumed the other's "Order" (or "Customer," "Account," "Product") meant the same thing, and it doesn't.
- A team is about to decompose a monolith into services (see `microservices-decomposition`) and needs the domain model, not the org chart or the database schema, to drive where the boundaries go.
- A new system needs to integrate with a legacy system or a third-party vendor whose model is known to be messy or actively wrong, and the team needs a way to protect their own model from that mess.
- Two teams are duplicating logic because nobody has mapped which team is upstream (defines the model) and which is downstream (consumes/translates it).
- An "enterprise-wide shared model" effort has stalled because trying to force one model to serve every department's needs is producing an unusable, everything-optional schema.
- Onboarding a new engineer takes weeks because the domain vocabulary is inconsistent across teams and nobody has written down what each term means in each context.

## Workflow

1. **Run event storming (or an equivalent domain-mapping workshop) with domain experts, not just engineers.** Capture domain events (`OrderPlaced`, `PaymentDeclined`) in the language the business actually uses, on a timeline, before mapping to any technical structure.
2. **Cluster events and terms into candidate bounded contexts** — places where the meaning and rules around a term stay consistent. A term changing meaning ("Product" means a catalog SKU in Catalog context, a billable line-item in Billing context) is the signal of a context boundary, not a modeling mistake to "fix" by unifying the term.
3. **Name the ubiquitous language per context explicitly.** Write down, per bounded context, what each key term means and what invariants hold — this becomes the shared vocabulary for that context's team, and the point of translation for any other context.
4. **Classify the relationship between every pair of contexts that integrate**, using the standard context-mapping patterns:
   - **Shared Kernel** — two teams deliberately share a small, jointly-owned model subset (use sparingly; it requires tight coordination on every change).
   - **Customer/Supplier** — downstream's needs influence upstream's roadmap (upstream has an obligation to consider downstream's requirements).
   - **Conformist** — downstream just accepts upstream's model as-is with no translation (fine when downstream has no leverage or the models genuinely align, e.g., consuming a well-run internal platform team's API).
   - **Anti-Corruption Layer (ACL)** — downstream translates upstream's model into its own at the boundary, protecting itself from upstream's model quality or churn (use this for legacy systems, third-party vendors, or any upstream you don't trust to stay stable).
   - **Open Host Service / Published Language** — upstream publishes a stable, well-documented API/schema intended for many consumers (the right pattern for a platform team serving many internal clients).
   - **Separate Ways** — two contexts that look related but have no real integration need; don't force one.
5. **Draw the context map** as a diagram showing each bounded context as a node and each relationship as a labeled edge (upstream/downstream direction plus the pattern from step 4) — this is the primary artifact, not a database ERD.
6. **Identify where an ACL is missing but needed.** If a downstream context is directly consuming an upstream model with no translation layer, and the upstream model is unstable, poorly documented, or a legacy/third-party system, flag it — this is the #1 source of "their bad model became our bad model" defects.
7. **Use the context map to drive service boundaries**, not the reverse — bounded contexts are candidate service boundaries (see `microservices-decomposition`), but a bounded context can also legitimately stay a module within a larger service if splitting it has no operational payoff yet.
8. **Reconcile the context map against team topology.** A bounded context owned by two separate teams with no clear customer/supplier agreement is a coordination bottleneck waiting to happen (Conway's Law) — either align team ownership to the context or make the relationship pattern (and whose roadmap wins on conflicts) explicit.
9. **Revisit the map when the domain changes**, not just once at project kickoff. A context map is a living document — new features often reveal that a term you thought was shared actually isn't, requiring the map (and possibly the service boundary) to be revised.
10. **Feed the map into integration design.** Each cross-context edge on the map should map to a concrete integration mechanism — synchronous API call, published event (see `event-driven-architecture-design`), or batch sync — decided per relationship, not uniformly.

## Example

Scenario: `NorthPeak Insurance` is integrating a new digital-quoting product with the legacy policy-administration system (`PolicyCore`, a 15-year-old mainframe-adjacent system) and an internal `Underwriting` team's risk-scoring service.

Event storming reveals three bounded contexts and the term "Policy" means something different in each:

```
Quoting context:      "Policy" = a draft, mutable quote with a premium estimate
PolicyCore (legacy):  "Policy" = a bound, immutable contract record with 40 legacy fields
Underwriting context: "Policy" = a risk profile with a score and rating factors
```

Context map:

| Relationship | Pattern | Reasoning |
|---|---|---|
| Quoting → PolicyCore | Anti-Corruption Layer | PolicyCore's model is a legacy mainframe schema with nulls-as-magic-values and undocumented field semantics; Quoting builds a translation layer that maps PolicyCore's messy fields into Quoting's own clean `Policy` model, isolating the rest of Quoting from legacy churn |
| Quoting → Underwriting | Customer/Supplier | Underwriting is actively developed in-house and has agreed to prioritize Quoting's requested fields in its API roadmap; no translation layer needed since Underwriting exposes a purpose-built API for Quoting |
| Underwriting → external credit-bureau API | Anti-Corruption Layer | Third-party vendor schema, versioned outside NorthPeak's control |
| Quoting internal pricing engine ↔ Quoting rating rules | Shared Kernel | Both owned by the same team, deliberately sharing a small `RatingFactors` value-object model — rejected splitting these into separate contexts since they change together on nearly every pricing update |

Rejected alternative: proposal to build one unified "Policy" service used by all three contexts. Rejected because PolicyCore's model has 40 legacy fields most of which are irrelevant to Quoting, and forcing Quoting to depend directly on PolicyCore's schema would mean every legacy quirk (e.g., a magic-number null convention) leaks into the new product — the ACL exists specifically to prevent this.

## Common pitfalls

- **Treating "Customer" (or any term) as one universal concept.** Forcing a single canonical model across contexts that genuinely need different views of the same real-world thing produces a bloated, mostly-optional schema nobody is happy with.
- **Consuming a legacy or third-party model directly with no ACL.** The upstream system's data quality problems, quirky field semantics, or breaking changes become your production incidents.
- **Confusing "shared kernel" with "just share the database."** Shared kernel means two teams jointly and deliberately own a small, tightly-coordinated model — not an accidental shared table that nobody agreed to co-own.
- **Skipping domain experts in the mapping exercise.** A context map drawn by engineers alone from the existing codebase just documents current accidental coupling instead of the real domain boundaries.
- **Drawing the map once and never updating it.** The domain changes as the business does; a stale context map gives false confidence about where the boundaries actually are.
- **Letting org chart dictate context boundaries** instead of the reverse — Conway's Law cuts both ways: if the context map says two things are one context but two teams own it, that's the actual problem to fix (via team realignment or an explicit customer/supplier agreement), not evidence the map is wrong.
- **Using "Separate Ways" as an excuse to skip needed integration** rather than a genuine finding that no real coupling exists between two contexts.

## Output

A context map document (e.g., `docs/architecture/context-map.md`) with a diagram of bounded contexts and labeled relationship edges (upstream/downstream direction + pattern), a per-context glossary of the ubiquitous language, and an explicit list of ACL locations with the reason each is needed — feeding directly into service-decomposition decisions (`microservices-decomposition`) and integration design.
