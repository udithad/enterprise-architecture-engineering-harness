+++
# EAEH skill (Markdown). The TOML front matter between the +++ lines is the metadata; the
# Markdown below it is the instruction text given to the agent, verbatim.
id = "architecture-diagrams"
version = "1.0.0"
description = "Consistent, reviewable architecture diagrams and component naming for HLD and LLD."
# Layers this skill applies to (stage task kinds). Once approved, EAEH adds it to every
# matching stage automatically:
#   requirements-analysis, requirements-review, hld-design, lld-design,
#   implementation-planning, implementation
task_kinds = ["hld-design", "lld-design"]
max_risk = "read_only"
evaluation_criteria = [
  "every component in the design appears in at least one diagram",
  "diagram node names match component ids",
]
+++

# Architecture diagrams

Apply these rules to the diagrams and component names you produce.

## Naming
- Use the **component id** (kebab-case, e.g. `product-service`) as the identifier everywhere: in components, interfaces, decisions, diagrams and sequences.
- Use the human-readable component **name** only as a label, never as an identifier.

## Coverage
- Every component must appear in at least one diagram. Every external actor or system must appear in the context view.
- Each sequence must start at an actor or an external system and use only participants that are defined components or actors.

## Clarity
- One concern per diagram: context, components, deployment or one flow. Keep each diagram under about 15 nodes. Split larger ones and say how the parts relate.
- Label every edge with the interaction (for example `GET /api/products`, `reads`, `publishes OrderPlaced`).
- Do not invent infrastructure (queues, caches, databases) that the requirements or the approved HLD do not justify. If one is needed, record it as a decision with its alternatives.
