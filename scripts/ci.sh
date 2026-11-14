#!/usr/bin/env bash
# EAEH quality gate. Usage: scripts/ci.sh
# Container isolation tests require Podman or Docker with docker.io/library/alpine:3.20
# pulled. To skip them explicitly (recorded as SKIPPED, never as passed):
#   EAEH_SKIP_CONTAINER_TESTS=1 scripts/ci.sh
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> toolchain"; rustc --version; cargo --version
echo "==> fmt";       cargo fmt --all -- --check
echo "==> clippy";    cargo clippy --workspace --all-targets --locked -- -D warnings
echo "==> build";     cargo build --workspace --locked
echo "==> test";      cargo test --workspace --locked --no-fail-fast
if command -v cargo-deny >/dev/null 2>&1; then
  echo "==> cargo-deny"; cargo deny --locked check
else
  echo "==> cargo-deny: NOT INSTALLED (supply-chain gate not executed; see ACTION-ITEMS A-07)"
fi
echo "==> CI PASSED"
