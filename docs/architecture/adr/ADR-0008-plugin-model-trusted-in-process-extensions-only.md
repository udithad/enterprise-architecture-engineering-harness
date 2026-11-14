# ADR-0008: Plugin model: trusted in-process extensions only

| Field | Value |
|---|---|
| Status | Accepted (D-03, clarification 1) |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
Plugins must not bypass authorization, policy, approvals or isolation. WASM and external-process hosts are deferred. Arbitrary untrusted Rust libraries must not be loaded dynamically.

## Decision
* Extensions implement `InProcessExtension`, are **compiled into the binary** and handed to `HarnessBuilder`. There is no dynamic loading.
* Manifests are validated (API compatibility, dependencies, resource limits) and integrity-checked (SHA-256 digest of the canonical manifest; ed25519 signature against configured trusted keys when `require_signature = true`, the default).
* A plugin contributes nothing until an administrator enables that exact version. Granted capabilities = requested ∩ configured `grantable`. Registration is all-or-nothing: a failure unregisters its hooks/tools and marks the plugin `failed`.
* The registration context exposes only the hook, tool and model registries — not the database, authorizer, policy engine or approval manager. Contributed tools are invoked only through the tool gateway.
* **Signatures establish provenance and integrity of the declaration, not runtime isolation**: an enabled in-process extension runs with the privileges of the harness process and must be trusted code.
* WASM and external-process manifests can be registered and validated but cannot be enabled (baseline policy deny + `NotSupported`).

## Consequences
Safe by construction for Phase 1. Untrusted third-party extensions require the deferred WASM/external hosts (A-12).
