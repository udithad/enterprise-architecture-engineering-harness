# Execution Isolation

See ADR-0009. Providers declare what they **genuinely** enforce; callers declare what they
need; selection and execution refuse when requirements are not met.

## Capability matrix (as implemented and tested)

| Capability | `restricted-process` | `container` (Podman/Docker CLI) | Evidence |
|---|---|---|---|
| Wall-clock timeout | ✔ process-group `SIGKILL` | ✔ `kill` + `rm -f` by name | `timeout_kills_entire_process_group`, `timeout_kills_and_removes_container` |
| Environment isolation | ✔ `env_clear` + allow-list | ✔ explicit `--env` only, `--http-proxy=false` (Podman) | `environment_is_cleared_to_allowlist`, `environment_contains_only_explicit_variables` |
| CPU limit | ✔ `RLIMIT_CPU` | ✔ `--cpus` | `cpu_time_limit_terminates_busy_loop` |
| File-size limit | ✔ `RLIMIT_FSIZE` | ✔ `--ulimit fsize` | `file_size_limit_enforced` |
| Filesystem confinement | ✘ declared not enforced | ✔ read-only rootfs, noexec `/tmp`, workspace-only mount | `filesystem_is_confined_to_workspace` |
| Network isolation | ✘ | ✔ `--network none` (loopback only; IP and DNS blocked) | `network_access_is_denied` |
| Memory limit | ✘ | ✔ `--memory` = `--memory-swap` (OOM kill, exit 137) | `memory_limit_is_enforced` |
| Process-count limit | ✘ | ✔ `--pids-limit` | `process_count_limit_is_enforced` |
| Privilege drop | ✘ | ✔ uid 65534, `CapEff/CapPrm = 0`, `NoNewPrivs = 1` | `runs_as_unprivileged_user_without_capabilities` |
| Bounded output | ✔ | ✔ | `output_is_bounded` |
| Refuses unsupported requirements | ✔ | n/a | `refuses_requirements_it_cannot_enforce`, `selection_refuses_instead_of_downgrading` |

Test environment of record: Podman 5.5.1, `podman-machine-default` (applehv, Fedora CoreOS
kernel 6.12.13), macOS 26 / Darwin 25.6.0 arm64, image `docker.io/library/alpine:3.20`.

## Usage in the platform

* `exec.run` requires all capabilities ⇒ only the container provider qualifies; when the
  container runtime is unavailable or disabled, `exec.run` is refused with `isolation`
  (verified by `isolation_requirements_refused_when_unavailable`).
* File tools run in-process; they need no process isolation because they never execute
  external code and are confined by `Workspace::resolve` (no absolute paths, no `..`, no
  symlinks on the path, `O_NOFOLLOW` reads, atomic writes, path locks).
* Workspaces live under `<data_dir>/workspaces/<tenant>/<project>/<id>` (mode `0700`). On
  macOS, container runtimes can mount only paths shared with their VM (by default under
  `/Users`); place `storage.data_dir` accordingly.
* Images are never pulled at execution time (`--pull never`); operators pre-pull and should
  pin images by digest in production configuration.

## Known limitations

| Id | Limitation |
|---|---|
| ISO-L1 | The restricted-process provider cannot confine filesystem, network, memory, pids or privileges; it is only selected for requirements it satisfies. |
| ISO-L2 | Docker injects proxy variables from `~/.docker/config.json`; there is no per-run switch (Podman is configured with `--http-proxy=false`). |
| ISO-L3 | On macOS virtiofs mounts, file ownership inside the workspace is not enforced (any container uid can write the workspace — intended writable area). |
| ISO-L4 | Container tests are fail-closed: they fail without a runtime unless `EAEH_SKIP_CONTAINER_TESTS=1` is set, in which case they print SKIPPED. |
| ISO-L5 | No seccomp/AppArmor profile customisation beyond runtime defaults; no microVM isolation (Phase 8 candidate). |
