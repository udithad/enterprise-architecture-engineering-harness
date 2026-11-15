#!/usr/bin/env bash
# EAEH Governance Portal with real EAEH data (ADR-0026).
#
# A throw-away installation under target/demo-portal in deterministic execution mode
# (recorded, hand-authored responses – NOT AI; no subscription or API key), onboarding
# `simple-springboot-api` like scripts/demo-onboarding.sh. Every approval is made by a
# human, in a separate step:
#
#   scripts/demo-portal.sh prepare         # installation, project, skills registered (pending)
#   scripts/demo-portal.sh approve-skills  # YOU: review and approve the skills (CLI, per item)
#   scripts/demo-portal.sh start           # start the design workflow, then run the portal
#
# `start` prints the portal link; open it and make the Requirements / HLD / LLD decisions
# yourself as the fixture's `approver` identity. Stop the portal with Ctrl-C;
# `scripts/demo-portal.sh portal` restarts it on the same data.
#
# Environment: EAEH_PORTAL_PORT (default 8190 – deliberately NOT the 8090 default of `eaeh portal`,
# so the demo never stands in for your real installation), EAEH_PORTAL_WORK (default target/demo-portal).
# The portal labels itself DEMO (EAEH_INSTALLATION_LABEL) in its banner and in the web UI.
# For `start`/`portal`, stdout carries only the portal launch URL (used by the browser E2E
# tests); progress goes to stderr.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"; WORK="${EAEH_PORTAL_WORK:-$ROOT/target/demo-portal}"
PORT="${EAEH_PORTAL_PORT:-8190}"
B="$ROOT/target/debug/eaeh"
export EAEH_HOME="$WORK/home" EAEH_CONFIG="$WORK/eaeh.toml"
unset EAEH_TOKEN EAEH_PROFILE || true
REPO="$WORK/simple-springboot-api"
cmd="${1:-}"

need_prepared() {
  [[ -f "$EAEH_CONFIG" && -d "$REPO" ]] || { echo "run 'scripts/demo-portal.sh prepare' first" >&2; exit 2; }
}

build_ui() {
  if [[ ! -f "$ROOT/portal/dist/index.html" || "${EAEH_PORTAL_REBUILD:-0}" == "1" ]]; then
    echo "== building the portal web UI (npm)" >&2
    [[ -d "$ROOT/portal/node_modules" ]] || npm --prefix "$ROOT/portal" ci --no-audit --no-fund >&2
    npm --prefix "$ROOT/portal" run build >&2
  fi
}

run_portal() {
  echo "== DEMO portal (throw-away installation $WORK, NOT your real EAEH data): viewing as 'developer', deciding as 'approver' – open the link below" >&2
  cd "$REPO"
  export EAEH_INSTALLATION_LABEL="DEMO – $WORK"
  exec "$B" portal --as developer --approver approver --bind "127.0.0.1:$PORT" --static-dir "$ROOT/portal/dist"
}

case "$cmd" in
  prepare)
    rm -rf "$WORK"; mkdir -p "$WORK" "$REPO"
    cargo build -q -p eaeh-cli
    build_ui
    cat > "$EAEH_CONFIG" <<EOF
[storage]
data_dir = "$WORK/data"
[definitions]
prompts_dir = "$ROOT/prompts"
agents_dir = "$ROOT/config/agents"
workflows_dir = "$ROOT/workflows"
policies_dir = "$ROOT/config/policies"
skills_dir = "$ROOT/config/skills"
[isolation]
container_enabled = false
[log]
format = "pretty"
filter = "warn"
[execution]
mode = "deterministic"
[providers.replay]
enabled = true
cassette = "$ROOT/examples/simple-springboot-api/replay-cassette.json"
EOF
    cp "$ROOT/examples/simple-springboot-api/BRD-Simple-Spring-Boot-API.md" "$ROOT/examples/simple-springboot-api/PROJECT-BOOTSTRAP.md" "$REPO/"
    cd "$REPO"
    echo "== platform and project setup (administrator)"
    "$B" init > /dev/null
    "$B" admin bootstrap --tenant acme --project platform --admin-id admin --token-file "$WORK/admin.token" > /dev/null
    "$B" auth login --profile admin --token-file "$WORK/admin.token" > /dev/null
    rm "$WORK/admin.token"
    set +e
    "$B" project init --name simple-springboot-api \
      --description "Simple REST API for local development and EAEH lifecycle demonstration" \
      --language "Java 21" --framework "Spring Boot 3.x" --build-tool Maven \
      --constraint "Products in memory; no database" --constraint "Local development only" \
      --brd BRD-Simple-Spring-Boot-API.md --knowledge PROJECT-BOOTSTRAP.md \
      --developer dev --steward steward --approver approver \
      --mode deterministic --client claude-code \
      --workflow requirements.analysis --workflow engineering.design --store-profiles > /dev/null
    set -e
    "$B" auth status
    "$B" skill pending
    echo
    echo "Next (human approver): scripts/demo-portal.sh approve-skills"
    ;;
  approve-skills)
    need_prepared; cd "$REPO"
    # Interactive per-item confirmation by the human approver (ADR-0024).
    "$B" approval review --as approver --all
    echo; echo "Next: scripts/demo-portal.sh start"
    ;;
  start)
    need_prepared; build_ui; cd "$REPO"
    exec 3>&1 1>&2
    "$B" project readiness > /dev/null || { echo "project not ready – approve the skills first (scripts/demo-portal.sh approve-skills)"; exit 7; }
    echo "== start the design workflow as the developer; it stops at the Requirements gate"
    "$B" workflow run design --yes > /dev/null
    "$B" workflow status | head -8
    exec 1>&3 3>&-
    run_portal
    ;;
  portal)
    need_prepared; build_ui; run_portal
    ;;
  *)
    sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
