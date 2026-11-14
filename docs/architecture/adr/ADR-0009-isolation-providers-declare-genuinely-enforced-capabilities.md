# ADR-0009: Isolation providers declare genuinely enforced capabilities; OCI container provider

| Field | Value |
|---|---|
| Status | Accepted (D-07, clarifications 2–3) |
| Date | 2026-11-12 |
| Phase | 1 – Core Harness Foundation |
| Deciders | Product owner (approval of PHASE-01-ASSESSMENT), Principal architect |

## Context
Isolation must be replaceable through an abstraction, claims must be backed by security tests, and the gateway must reject operations when required capabilities are unavailable.

## Decision
* `ExecutionProvider::capabilities()` declares eight properties: timeout, env isolation, CPU limit, filesystem confinement, network isolation, memory limit, pid limit, privilege drop.
* `RestrictedProcessProvider` enforces only timeout (process-group `SIGKILL`), cleared environment, `RLIMIT_CPU`, `RLIMIT_FSIZE`, `RLIMIT_CORE=0` and bounded output — and declares the other five **not enforced**.
* `ContainerProvider` (Podman or Docker CLI): `--network none`, `--read-only` rootfs, `noexec` size-limited `/tmp`, workspace-only bind mount, memory = memory-swap, `--cpus`, `--pids-limit`, `--cap-drop ALL`, `no-new-privileges`, non-root numeric uid, `--pull never`, explicit `--env` only, `--http-proxy=false` (Podman). Verified by 7 escape-attempt tests on Podman 5.5.1 (macOS applehv VM).
* `ProviderRegistry::select` refuses rather than downgrades; each provider re-checks requirements; `exec.run` requires all eight capabilities plus a program allow-list.

## Consequences
Strong isolation depends on an available container runtime; without one, executing tools are refused (fail closed). Docker injects proxy variables configured in `~/.docker/config.json` and has no per-run switch (ISO-L2). On macOS virtiofs, file ownership inside the workspace mount is not enforced (the workspace is the intended writable area).

## Alternatives considered
`sandbox-exec` (deprecated on macOS); gVisor / Firecracker microVMs (Phase 8 candidates behind the same trait).
