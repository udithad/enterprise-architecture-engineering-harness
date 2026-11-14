# Demonstration knowledge corpus

**These files are demonstration fixtures for EAEH Phase 2, not enterprise standards.**
EAEH ships no enterprise standards (action A-09); an organisation supplies its own.
They exist so the knowledge → skill → agent flow can be exercised end to end
(`tests/tests/knowledge_demo.rs`, `scripts/demo-knowledge.sh`).

| File | Source (kind) | Ingested as |
|---|---|---|
| `standards/error-format.md` | `demo-api-standards` (standard) | enterprise, mandatory → needs approval |
| `guides/resource-naming.md` | `demo-api-guides` (reference) | enterprise |
| `guides/pagination.md` | `demo-api-guides` (reference) | enterprise |
| `specs/orders-api.json` | `demo-api-specs` (api_spec) | project-scoped OpenAPI document |
