//! Container provider security-boundary tests (clarification 3).
//!
//! These tests attempt real escapes inside a real container runtime. They are
//! **fail-closed**: if no runtime/image is available they FAIL, unless the operator sets
//! `EAEH_SKIP_CONTAINER_TESTS=1`, in which case they print an explicit SKIPPED notice.
//!
//! Configuration:
//! * `EAEH_CONTAINER_RUNTIME` – `podman` (default) or `docker`.
//! * `EAEH_CONTAINER_IMAGE` – default `docker.io/library/alpine:3.20` (must be pulled).

use eaeh_isolation::{
    ContainerConfig, ContainerProvider, ExecutionProvider, ExecutionSpec, IsolationCapabilities,
    ResourceLimits,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

async fn provider() -> Option<ContainerProvider> {
    let cfg = ContainerConfig {
        runtime: std::env::var("EAEH_CONTAINER_RUNTIME").unwrap_or_else(|_| "podman".into()),
        image: std::env::var("EAEH_CONTAINER_IMAGE")
            .unwrap_or_else(|_| "docker.io/library/alpine:3.20".into()),
        ..Default::default()
    };
    let p = ContainerProvider::new(cfg).unwrap();
    match p.probe().await {
        Ok(()) => Some(p),
        Err(e) if std::env::var("EAEH_SKIP_CONTAINER_TESTS").as_deref() == Ok("1") => {
            eprintln!("SKIPPED (EAEH_SKIP_CONTAINER_TESTS=1): {e}");
            None
        }
        Err(e) => panic!(
            "container runtime required for isolation tests: {e} (set EAEH_SKIP_CONTAINER_TESTS=1 to skip explicitly)"
        ),
    }
}

/// Workspace under the cargo target dir: inside $HOME, which Podman/Docker Desktop
/// machines share with the VM (system temp dirs are not shared on macOS).
fn workspace(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("ws-{name}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn sh(ws: &std::path::Path, script: &str) -> ExecutionSpec {
    ExecutionSpec {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        env: [("EAEH_TASK".to_string(), "iso-test".to_string())].into(),
        workspace: ws.to_path_buf(),
        limits: ResourceLimits {
            timeout: Duration::from_secs(60),
            memory_bytes: 64 * 1024 * 1024,
            max_pids: 16,
            ..Default::default()
        },
        requirements: IsolationCapabilities::all(),
    }
}

#[tokio::test]
async fn runs_as_unprivileged_user_without_capabilities() {
    let Some(p) = provider().await else { return };
    let ws = workspace("priv");
    let out = p
        .execute(sh(
            &ws,
            "id -u; grep -E '^(CapEff|CapPrm|NoNewPrivs)' /proc/self/status",
        ))
        .await
        .unwrap();
    assert!(out.success(), "{out:?}");
    assert!(out.stdout.lines().next() == Some("65534"), "{}", out.stdout);
    assert!(out.stdout.contains("CapEff:\t0000000000000000"), "{}", out.stdout);
    assert!(out.stdout.contains("CapPrm:\t0000000000000000"), "{}", out.stdout);
    assert!(out.stdout.contains("NoNewPrivs:\t1"), "{}", out.stdout);
}

#[tokio::test]
async fn network_access_is_denied() {
    let Some(p) = provider().await else { return };
    let ws = workspace("net");
    let script = "(wget -q -T 3 -O- http://1.1.1.1 >/dev/null 2>&1 && echo IP_OPEN) || echo ip_blocked; \
                  (nslookup example.com >/dev/null 2>&1 && echo DNS_OPEN) || echo dns_blocked; \
                  ip -o link | awk -F': ' '{print $2}'";
    let out = p.execute(sh(&ws, script)).await.unwrap();
    assert!(out.stdout.contains("ip_blocked"), "{}", out.stdout);
    assert!(out.stdout.contains("dns_blocked"), "{}", out.stdout);
    let ifaces: Vec<&str> = out.stdout.lines().skip(2).collect();
    assert_eq!(ifaces, vec!["lo"], "only loopback expected: {}", out.stdout);
}

#[tokio::test]
async fn filesystem_is_confined_to_workspace() {
    let Some(p) = provider().await else { return };
    let ws = workspace("fs");
    std::fs::write(ws.join("input.txt"), "hello").unwrap();
    let script = "(touch /etc/pwned 2>/dev/null && echo ROOT_WRITABLE) || echo root_readonly; \
                  (touch /usr/pwned 2>/dev/null && echo USR_WRITABLE) || echo usr_readonly; \
                  (ls /Users /home/core /var/home 2>/dev/null | head -1 | grep -q . && echo HOST_VISIBLE) || echo host_hidden; \
                  cat input.txt; echo; echo produced > output.txt && echo ws_write_ok; \
                  cp /bin/busybox /tmp/bb 2>/dev/null; (/tmp/bb true 2>/dev/null && echo TMP_EXEC) || echo tmp_noexec";
    let out = p.execute(sh(&ws, script)).await.unwrap();
    for expect in [
        "root_readonly",
        "usr_readonly",
        "host_hidden",
        "hello",
        "ws_write_ok",
        "tmp_noexec",
    ] {
        assert!(out.stdout.contains(expect), "missing {expect}: {}", out.stdout);
    }
    assert_eq!(
        std::fs::read_to_string(ws.join("output.txt")).unwrap().trim(),
        "produced"
    );
}

#[tokio::test]
async fn memory_limit_is_enforced() {
    let Some(p) = provider().await else { return };
    let ws = workspace("mem");
    let out = p
        .execute(sh(
            &ws,
            "x=$(head -c 200000000 /dev/zero | tr '\\0' a); echo ALLOCATED ${#x}",
        ))
        .await
        .unwrap();
    assert!(
        !out.stdout.contains("ALLOCATED"),
        "allocation beyond limit succeeded: {out:?}"
    );
    assert_eq!(out.exit_code, Some(137), "expected OOM kill (137): {out:?}");
}

#[tokio::test]
async fn process_count_limit_is_enforced() {
    let Some(p) = provider().await else { return };
    let ws = workspace("pids");
    let out = p
        .execute(sh(
            &ws,
            "n=0; for i in $(seq 1 40); do sleep 5 & n=$((n+1)); done; echo spawned=$n",
        ))
        .await
        .unwrap();
    assert!(!out.stdout.contains("spawned=40"), "{out:?}");
    assert!(
        out.stderr.contains("can't fork") || out.stderr.contains("Resource temporarily unavailable"),
        "{out:?}"
    );
}

#[tokio::test]
async fn timeout_kills_and_removes_container() {
    let Some(p) = provider().await else { return };
    let ws = workspace("timeout");
    let mut s = sh(&ws, "echo begin; sleep 120");
    s.limits.timeout = Duration::from_secs(3);
    let start = Instant::now();
    let out = p.execute(s).await.unwrap();
    assert!(out.timed_out, "{out:?}");
    assert!(
        start.elapsed() < Duration::from_secs(45),
        "took {:?}",
        start.elapsed()
    );
    let ps = tokio::process::Command::new(&p.config().runtime)
        .args([
            "ps",
            "-a",
            "--filter",
            "name=eaeh-",
            "--format",
            "{{.Names}} {{.Command}}",
        ])
        .output()
        .await
        .unwrap();
    let listing = String::from_utf8_lossy(&ps.stdout);
    assert!(
        !listing.contains("sleep 120"),
        "timed-out container still present: {listing}"
    );
}

#[tokio::test]
async fn environment_contains_only_explicit_variables() {
    let Some(p) = provider().await else { return };
    let ws = workspace("env");
    let out = p.execute(sh(&ws, "env | cut -d= -f1 | sort")).await.unwrap();
    let keys: Vec<&str> = out.stdout.lines().collect();
    for k in &keys {
        assert!(
            [
                "EAEH_TASK",
                "HOME",
                "PATH",
                "HOSTNAME",
                "SHLVL",
                "PWD",
                "container"
            ]
            .contains(k),
            "unexpected env var {k}: {keys:?}"
        );
    }
    assert!(!out.stdout.contains("CARGO"), "host environment leaked");
}
