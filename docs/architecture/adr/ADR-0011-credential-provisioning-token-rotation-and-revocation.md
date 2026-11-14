# ADR-0011: Credential provisioning, token rotation and revocation

| Field | Value |
|---|---|
| Status | Accepted (clarification 4) |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
Phase 1 must define secure initial administrator/API credential provisioning, token rotation and revocation, and must never introduce default hard-coded credentials.

## Decision
* No default credentials exist. `eaeh admin bootstrap` (`IdentityService::bootstrap`) creates the first tenant, project and administrator **only while no principal exists** (checked inside the serialized write transaction) and returns a fresh token exactly once — printed, or written to a new file with mode `0600` (`--token-file`; refuses to overwrite).
* Tokens are `eaeh_` + 256-bit OS-random hex. Only SHA-256 hashes are stored. Expiry 1–365 days (default 30); revocation; rotation issues a new token and revokes the old one atomically (`replaced_by`); disabled principals cannot authenticate; all failures return an indistinguishable `unauthenticated`.
* Principals manage their own tokens (`token.manage_own`); administrators manage others in their tenant only (`token.manage_any`).
* The CLI reads the token from `EAEH_TOKEN`, never from arguments. Issued tokens are registered with the redactor and `eaeh_…` patterns are redacted from logs, audit records and agent context.

## Consequences
Bootstrap requires file-system access to the data directory (the trust root). Enterprise identity (OIDC/SAML) is an extension point for later phases (A-14).
