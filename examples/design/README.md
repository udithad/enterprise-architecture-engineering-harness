# Demonstration design fixtures (Phase 3)

**These are demonstration fixtures, not real requirements or designs.** They match the
labelled demonstration BRD `examples/brd/order-management.md` (ingested as
`demo-brd/order-management@v1`) and the demonstration standard
`demo-api-standards/error-format@v1`.

| File | Used by |
|---|---|
| `requirements.json` | `crates/eaeh-architecture/tests`, `tests/tests/design_demo.rs` (scripted business analyst / requirements engineer output) |
| `hld.json` | same (scripted solution architect output) |
| `lld.json` | same (scripted software designer output) |

They pass all deterministic checks (`architecture.rs::demonstration_fixtures_pass_all_deterministic_checks`).
They are **not** model output: no live model run has been performed in Phase 3 (A-30).
