# EAEH Setup Automation (`eaeh setup`)

Scope: Phases 1–3 (CLI/developer experience over existing identity, skills, governance and portal services). No roadmap, ADR or acceptance-criteria change. Plan: [SETUP-AUTOMATION-PLAN.md](stages/SETUP-AUTOMATION-PLAN.md) (approved with the recommended options: detached portal, tenant `local` / project `default`, reset removes only the database files).

User-facing documentation: [EAEH User Guide, section 3 and 4](../user-guides/EAEH-USER-GUIDE.md).

## Implementation

| File | Change |
|---|---|
| `crates/eaeh-cli/src/setup.rs` (new) | The whole feature: check engine and report, identity matrix, backup/restore, reset, skill orchestration, detached portal control |
| `crates/eaeh-cli/src/main.rs` | `Cmd::Setup` and its options; dispatch |
| `crates/eaeh-cli/src/dx.rs` | `store_issued` made `pub(crate)` (reused to store credentials without displaying them) |
| `crates/eaeh-cli/Cargo.toml` | `eaeh-store` as dev-dependency only (tests) |
| `docs/implementation/EAEH-PORTAL-CLI-PARITY.md` | `eaeh setup` classified as CLI-only |
| `crates/eaeh-runtime/src/maintenance.rs` (new) | Verified database backup (`VACUUM INTO`) and setup audit helpers (fitness rule: the CLI may not depend on `eaeh-store`) |
| `crates/eaeh-cli/tests/setup.rs` (new) | 22 end-to-end tests |
| `docs/user-guides/EAEH-USER-GUIDE.md` | Sections 3, 4, 6.3, 9, 11 rewritten / extended; manual bootstrap steps removed |

No new identity, approval or skill service: setup calls `IdentityService` (`bootstrap`, `create_principal`, `grant_project_access`, `set_principal_status`, `issue_token`, `authenticate_detailed`), `eaeh_runtime::project::{discover_skills, sync_skills}`, the credential store, the coordinator preflight and the existing `eaeh portal` process.

## Commands

`eaeh setup [--check] [--repair] [--reset] [--confirm-reset "RESET EAEH"] [--restore <dir>] [--open] [--no-portal] [--stop-portal] [--bind 127.0.0.1:<port>] [--static-dir <dir>] [--tenant <n>] [--project <n>] [--ttl-days <n>]` (global `--config`, `--json`). Exit codes follow existing conventions: 0 ok, 7 something mandatory not ready, 1 usage error.

## Behaviour

* **Order:** reset/restore (confirmation, portal stopped, verified backup) → database/health/audit chain → first-time bootstrap (only when the database has no principals) → acting administrator (the stored `admin` profile, verified) → project → each identity in the matrix → project-permission summary → workflow definitions and agents → skills → external-AI/worker/MCP → portal (only when nothing is `FAILED`).
* **Identity verification:** a profile is valid only if its token authenticates as the intended principal *and* that principal's roles are exactly the intended one *and* its effective permissions match the matrix (e.g. `developer` may `workflow.start`, may not `principal.manage` / `approval.decide`). Permissions come from the server-side principal, never from stored profile metadata.
* **Safe repairs without a flag:** missing principal (created by the administrator), missing project access (granted), missing/expired/revoked credential (new token issued, stored under the profile). **Only with `--repair`:** profile mapped to another principal or to another installation (credential file backed up first), disabled standard principal re-enabled.
* **Never done:** changing an existing principal's roles (no service exists; reported as blocked, with the security wording when a non-admin holds `admin`), approving anything, inventing an administrator credential.
* **No loops:** every repair is attempted once, then verified; a failed verification is `FAILED` with remediation.
* **Audit:** `setup.run` (system actor, scoped to the project; lists repairs, blocked areas, skill counts) and `setup.credential_profile` (principal, reason; never a token). Principal/token/project/grant changes are audited by the identity service itself.
* **Skills:** `discover_skills` (directories; tree-hash; changed content becomes a new pending version, history preserved) plus `sync_skills` for file skills required by the requirements/design/delivery workflows. Counts are derived from the registry before/after, not from the call results. All new versions are `pending_approval`; the report states they are not available to agents or workflows until a human approves.
* **Portal:** spawns this binary as `eaeh --as admin portal … --approver approver --steward steward --operator developer --admin admin` in its own process group; output to `$EAEH_HOME/portal/portal-<id>.{out,log}` (0600); state (pid, address, launch link) in `portal-<id>.json` (0600). Reuse requires pid alive, process is `eaeh … portal` and HTTP answers. A listening port without a matching state is reported as a port conflict.
* **Backups:** `$EAEH_HOME/backups/<UTC>/` (0700): `eaeh.db` (`VACUUM INTO`, integrity-checked), `credentials.json`, `eaeh.toml`, `MANIFEST.json` (SHA-256). Credential-only backups `<UTC>-credentials/` precede any replacement of a stored profile.

## Verification evidence

Executed in this session on the real binary, real SQLite, real HTTP, isolated `EAEH_HOME` and data directory (the development database and credentials were not touched):

* `cargo test -p eaeh-cli --test setup`: **22 passed, 0 failed**.
* `cargo clippy -p eaeh-cli --all-targets -- -D warnings`: clean. `cargo fmt -p eaeh-cli -- --check`: clean.
* Full workspace result: see the final report (`cargo test --workspace --locked --no-fail-fast`).
* Manual run (scratch installation, port 18761): first run (tenant, project, 4 identities, 54 skills pending, portal started), repeat (nothing repaired, portal reused), revoked developer token (repaired), `approver → admin` mapping (blocked, then repaired with `--repair`), reset without confirmation (refused), reset with confirmation (backup, fresh installation, user directories kept), restore, port conflict (reported), interactive A/B prompt in a real pseudo-terminal (A default; B + typed phrase).

| Requested test | Test |
|---|---|
| 1 First-time setup | `first_time_setup_creates_identities_credentials_skills` |
| 2 Repeat without duplication | `repeated_setup_does_not_duplicate_anything` |
| 3 Existing healthy environment | `check_on_healthy_environment_changes_nothing`, `check_on_missing_installation_…` |
| 4 Missing principals | `missing_principals_are_created_by_the_administrator` |
| 5 Wrong profile mapping | `profile_mapped_to_wrong_principal_…`, `developer_can_never_end_up_as_admin` |
| 6 Expired / revoked credentials | `revoked_and_expired_credentials_are_reissued` |
| 7 Wrong role | `principal_with_wrong_roles_blocks_setup_and_portal` |
| 8 Missing project permissions | `missing_project_access_is_granted`, `standard_people_get_access_to_every_project_of_the_tenant` |
| 9 Missing admin credential | `missing_admin_credential_fails_safely_without_changes` |
| 10 Reset with confirmation | `reset_needs_confirmation_backs_up_and_is_recoverable` |
| 11 Backup and recovery | same test (restore), `restore_refuses_a_tampered_backup` |
| 12–14 Skills: discovery, change, pending | `skills_are_registered_pending_and_changes_are_detected`, `invalid_skill_is_reported_not_registered` |
| 15 Portal start (real HTTP, session, identities) | `portal_starts_in_background_…`, `portal_runs_with_the_standard_identities` |
| 16 Portal already running | `portal_starts_in_background_…` (same pid reused) |
| 17 Port conflict | `port_conflict_is_reported_and_not_mistaken_for_the_portal` |
| 18 Failed init and recovery | `failed_initialisation_changes_nothing_and_setup_recovers`, `interrupted_reset_recovers_on_the_next_run` |

Expired and wrongly-roled principals cannot be produced through any CLI command, so those tests write the state directly into the test database (`UPDATE api_tokens …`, `UPDATE principals …`), exactly as a degraded installation would look; the command under test is still the real binary.

## Known limitations

1. **Lost administrator token ⇒ reset only.** No service can issue an admin credential without an admin; by design nothing bypasses this.
2. **Roles are never changed.** A principal with wrong roles (including a non-admin holding `admin`) is reported `FAILED`; recovery is reset or choosing other principals. A role-change service would be a new governance feature and was not added.
3. **`eaeh setup --check` is read-only for identities, credentials and skills**, but opening the database applies pending migrations, token authentication updates `last_used_at`, and the run writes a `setup.run` audit record.
4. **External AI** readiness shows the client, version and login type; a working sign-in cannot be proved without a model call (forbidden in `external_ai` mode, ADR-0025). Shown as ready/not ready, never as verified.
5. **MCP:** only the availability of the `eaeh mcp` binary is checked; per-project `.mcp.json` is written and validated by `eaeh project init` / `project readiness`.
6. **Portal supervision:** a detached process, not a service; it does not restart after reboot (run `eaeh setup`). Windows is not supported by the process control (uses `kill`, `ps`, process groups).
7. **Reset while `eaeh serve` or a worker is using the database** is not detected (only the setup-started portal is stopped).
8. **The launch link is printed** (as `eaeh portal` already does); it carries a one-time portal secret, not an identity token.
9. **Setup creates a project `default`** if absent, even in an older installation whose project has another name (override with `--project`).
10. The web UI must be built (`portal/dist`); setup does not run npm.
11. The interactive A/B prompt was verified manually in a pseudo-terminal, not by an automated test.
