//! Restricted-process provider: verifies what it claims to enforce, and that it refuses
//! requirements it cannot enforce.

use eaeh_isolation::{
    ExecutionProvider, ExecutionSpec, IsolationRequirements, ResourceLimits, RestrictedProcessProvider,
};
use std::time::{Duration, Instant};

fn spec(dir: &std::path::Path, program: &str, args: &[&str]) -> ExecutionSpec {
    ExecutionSpec {
        program: program.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: [("EAEH_TASK".to_string(), "t1".to_string())].into(),
        workspace: dir.to_path_buf(),
        limits: ResourceLimits {
            timeout: Duration::from_secs(10),
            ..Default::default()
        },
        requirements: IsolationRequirements {
            timeout: true,
            env_isolation: true,
            ..Default::default()
        },
    }
}

#[tokio::test]
async fn environment_is_cleared_to_allowlist() {
    let d = tempfile::tempdir().unwrap();
    let out = RestrictedProcessProvider::new()
        .execute(spec(d.path(), "/usr/bin/env", &[]))
        .await
        .unwrap();
    assert!(out.success(), "{out:?}");
    let mut keys: Vec<&str> = out.stdout.lines().filter_map(|l| l.split('=').next()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["EAEH_TASK", "HOME", "PATH"],
        "only allowlisted variables: {}",
        out.stdout
    );
    assert!(!out.stdout.contains(".cargo"), "host PATH must not leak");
}

#[tokio::test]
async fn timeout_kills_entire_process_group() {
    let d = tempfile::tempdir().unwrap();
    let mut s = spec(d.path(), "/bin/sh", &["-c", "sleep 30 & echo started; wait"]);
    // 2 s leaves room for shell start-up under parallel test load (a 500 ms limit was
    // flaky); a surviving `sleep 30` grandchild would still blow the 5 s bound below.
    s.limits.timeout = Duration::from_secs(2);
    let start = Instant::now();
    let out = RestrictedProcessProvider::new().execute(s).await.unwrap();
    assert!(out.timed_out);
    assert!(out.stdout.contains("started"), "{out:?}");
    // If the backgrounded grandchild survived it would hold stdout open for 30s.
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn cpu_time_limit_terminates_busy_loop() {
    let d = tempfile::tempdir().unwrap();
    let mut s = spec(d.path(), "/bin/sh", &["-c", "while :; do :; done"]);
    s.limits.cpu_seconds = 1;
    s.limits.timeout = Duration::from_secs(20);
    let out = RestrictedProcessProvider::new().execute(s).await.unwrap();
    assert!(
        !out.timed_out,
        "must be stopped by RLIMIT_CPU, not the wall clock: {out:?}"
    );
    assert!(out.signal.is_some() || out.exit_code != Some(0), "{out:?}");
}

#[tokio::test]
async fn file_size_limit_enforced() {
    let d = tempfile::tempdir().unwrap();
    let mut s = spec(
        d.path(),
        "/bin/dd",
        &["if=/dev/zero", "of=big.bin", "bs=1024", "count=200"],
    );
    s.limits.max_file_bytes = 10 * 1024;
    let out = RestrictedProcessProvider::new().execute(s).await.unwrap();
    assert!(!out.success(), "{out:?}");
    let size = std::fs::metadata(d.path().join("big.bin"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(size <= 10 * 1024, "file grew to {size}");
}

#[tokio::test]
async fn output_is_bounded() {
    let d = tempfile::tempdir().unwrap();
    let mut s = spec(d.path(), "/usr/bin/yes", &[]);
    s.limits.max_output_bytes = 1000;
    s.limits.timeout = Duration::from_millis(300);
    let out = RestrictedProcessProvider::new().execute(s).await.unwrap();
    assert!(out.stdout_truncated);
    assert!(out.stdout.len() <= 1000);
}

#[tokio::test]
async fn refuses_requirements_it_cannot_enforce() {
    let d = tempfile::tempdir().unwrap();
    for req in [
        IsolationRequirements {
            network_isolation: true,
            ..Default::default()
        },
        IsolationRequirements {
            filesystem_confinement: true,
            ..Default::default()
        },
        IsolationRequirements {
            memory_limit: true,
            ..Default::default()
        },
        IsolationRequirements {
            privilege_drop: true,
            ..Default::default()
        },
    ] {
        let mut s = spec(d.path(), "/bin/echo", &["should not run"]);
        s.requirements = req;
        let e = RestrictedProcessProvider::new().execute(s).await.unwrap_err();
        assert_eq!(e.kind, eaeh_core::ErrorKind::Isolation, "{e}");
    }
}
