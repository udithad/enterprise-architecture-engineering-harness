---
name: security-architecture-review
description: "Systematically reviews a system's architecture for authentication/authorization gaps, trust-boundary violations, and unencrypted sensitive data paths before it ships. Use when a new system or major feature is being designed, before a security-sensitive launch, or as a recurring architecture-level (not just code-level) security check."
---

# Security Architecture Review

Systematically walk a system's architecture — not its code — for authentication and authorization design gaps, trust-boundary violations, and sensitive-data exposure paths, catching design-level security flaws that a code-level review or a scanner can't see because they're structural, not a single bad line.

## When to use this

- A new system or a major new feature is being designed and needs a security pass before implementation locks in the architecture.
- Before a launch handling sensitive data (PII, payment data, health data) or a launch into a new trust context (exposing an internal system to external partners or the public internet).
- As a recurring, scheduled review for systems that have grown organically — trust boundaries erode over time as new integrations are added without anyone re-checking the original assumptions.
- Preparing for `architecture-review-board-prep` where security posture needs to be presented with evidence, not just asserted.
- After a security incident, to check whether the same class of architectural gap exists elsewhere in the system.

## Workflow

1. **Draw the trust boundaries explicitly** on a diagram (reuse `c4-model-diagramming` or `system-context-diagram-authoring`) — every place where data crosses from a less-trusted zone to a more-trusted one (internet → edge, edge → internal network, internal service → another team's service, service → third-party API). Most architectural security bugs live at an unmarked or wrongly-assumed trust boundary.
2. **For every trust boundary, ask what enforces the boundary** — is it a network control (firewall, VPC boundary), an authentication check, an authorization check, or nothing? "Nothing" at a boundary that looks protected by network topology alone is a common latent flaw once that topology assumption breaks (a new peering connection, a cloud misconfiguration).
3. **Review authentication design**: what proves identity at each boundary, whether service-to-service calls are authenticated (not just "it's on the internal network so it's trusted") and whether credentials/tokens are scoped and short-lived rather than long-lived static secrets passed everywhere.
4. **Review authorization design separately from authentication** — being authenticated (who you are) is not being authorized (what you're allowed to do); check that authorization decisions happen close to the resource being protected, not only at the edge, and that they use the principle of least privilege rather than broad roles granted for convenience.
5. **Trace sensitive data flows end to end**: where does PII/payment/secret data originate, every system it passes through, every place it's stored (including logs, caches, and backups — frequently missed), and whether it's encrypted in transit and at rest at every hop, not just the primary datastore.
6. **Check for the confused deputy problem**: does any service perform an action on a caller's behalf using its own broader privileges, without verifying the caller was actually authorized for that specific action? This is a common source of privilege escalation in service-to-service architectures.
7. **Review secrets and credential management** (see `secrets-and-credential-management`): are secrets in config files, environment variables, or a proper secrets manager with rotation; is there any credential that, if leaked, grants broader access than the specific integration needs?
8. **Assess blast radius per component** — if this component is fully compromised, what can an attacker reach from there? Design should minimize blast radius (network segmentation, scoped credentials) rather than architectures where compromising one low-value service grants a path to everything.
9. **Check third-party and dependency trust** — what data or access is granted to third-party integrations, webhooks, and SDKs, and is it the minimum needed, with a defined revocation path if the third party is later compromised or the integration is deprecated.
10. **Document findings with severity and a remediation owner**, not just a list of concerns — an unranked, unowned list of findings tends to produce no action; a prioritized list with owners produces fixes.

## Example

Scenario: `ClinicSync`, a healthcare scheduling platform, is adding a new feature letting third-party billing partners pull appointment and diagnosis-code data via an API, ahead of a HIPAA-relevant launch.

Review findings:
- **Trust boundary gap**: the new partner API reused the same internal service-to-service authentication token used between ClinicSync's own microservices — meaning a partner's leaked API key would grant the same broad internal access as an internal service, not a scoped external one. **Finding: high severity.** Remediation: issue partner-specific, scoped OAuth tokens limited to the specific endpoints and fields a given partner is contracted for.
- **Authorization at the wrong layer**: the partner API checked "is this a valid partner token" at the edge gateway, but the underlying appointment service then returned full patient records including diagnosis codes regardless of which specific patients the partner was authorized to bill for. **Finding: high severity — confused deputy pattern.** Remediation: authorization narrowed to the specific patient-partner billing relationship, enforced in the appointment service itself, not assumed satisfied by the edge check.
- **Sensitive data in logs**: diagnosis codes were being logged in plaintext in the API gateway's access logs for debugging. **Finding: medium severity.** Remediation: redact diagnosis codes and other PHI fields from logs; if needed for debugging, log a reference ID instead.
- **Unscoped blast radius**: the partner-facing service ran with the same database credentials as the full internal appointment service (read/write to all tables), rather than a read-only, field-scoped credential. **Finding: high severity.** Remediation: separate, minimally-scoped read replica credential for the partner-facing path.
- **Third-party revocation gap**: there was no way to revoke a single partner's access without rotating credentials for all partners. **Finding: medium severity.** Remediation: per-partner credential issuance so a single partner can be revoked independently.

All findings logged with severity, owning team, and target remediation date, and re-reviewed before the launch date rather than accepted as known debt at launch.

## Common pitfalls

- **Assuming network location implies trust** ("it's internal, so it's fine") without an explicit authentication/authorization check at the boundary.
- **Authenticating but not authorizing** — checking who a caller is but not narrowly checking what that specific caller is allowed to do to that specific resource.
- **The confused deputy pattern** — a service using its own broad privileges to act on a caller's behalf without verifying the caller's specific authorization.
- **Sensitive data leaking into logs, caches, or backups** that weren't in scope of the "primary datastore is encrypted" review.
- **Long-lived, broadly-scoped credentials** shared across many integrations, so one leak compromises far more than necessary.
- **No blast-radius thinking** — a low-value component's compromise turns out to be a path to everything because of shared credentials or flat network access.
- **Findings recorded without severity or an owner**, producing a list that never gets acted on.

## Output

A security architecture review report: an annotated trust-boundary diagram, a list of findings each with severity, the specific architectural gap, remediation recommendation, and an owner/target date — reviewed and re-checked before the relevant launch or on a recurring schedule.
