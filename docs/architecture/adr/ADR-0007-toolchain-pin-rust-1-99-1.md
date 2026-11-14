# ADR-0007: Toolchain pin: Rust 1.99.1

| Field | Value |
|---|---|
| Status | Accepted (D-06) |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
D-06 requires a currently supported stable toolchain (not 1.86) with the exact version recorded and reproducible builds.

## Decision
`rust-toolchain.toml` pins channel `1.99.1` with clippy and rustfmt. Workspace `rust-version = 1.99`, edition 2024, `Cargo.lock` committed, CI runs with `--locked`.

## Consequences
Builds are reproducible for a given lockfile; toolchain upgrades are explicit, reviewed changes.
