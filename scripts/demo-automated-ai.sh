#!/usr/bin/env bash
# EAEH automated external-AI acceptance run (ADR-0026) on `simple-springboot-api`.
#
# The AI work runs automatically in a background worker through the installed Claude Code
# CLI, signed in with YOUR Claude subscription (no API key; EAEH makes no model calls).
# People only make the governance decisions:
#
#   scripts/demo-automated-ai.sh setup            # platform + project bootstrap (no approvals)
#   scripts/demo-automated-ai.sh approve-skills   # HUMAN: approver reviews and approves the skills
#   scripts/demo-automated-ai.sh start            # developer starts `design`; the worker takes over
#   scripts/demo-automated-ai.sh status           # optional: progress, worker, executions
#   scripts/demo-automated-ai.sh review           # HUMAN: writes the document under review, shows the decide command
#   scripts/demo-automated-ai.sh decide approve "<comments>"            # HUMAN approver decision
#   scripts/demo-automated-ai.sh decide request_changes "<what to change>"
#   scripts/demo-automated-ai.sh report           # evidence: executions, approvals, audit, model usage
#   scripts/demo-automated-ai.sh stop             # stop the background worker
#
# Works on a throw-away copy under target/demo-automated-ai; your repositories are not
# modified. A model provider (replay) is enabled in the configuration on purpose: external_ai
# mode must ignore it (no fallback).
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"; WORK="$ROOT/target/demo-automated-ai"; REPO="$WORK/simple-springboot-api"
B="$ROOT/target/debug/eaeh"
export EAEH_HOME="$WORK/home" EAEH_CONFIG="$WORK/eaeh.toml"
unset EAEH_TOKEN EAEH_PROFILE || true
step() { echo; echo "== $*"; }

case "${1:-}" in
setup)
  if [ -d "$WORK/data" ]; then "$B" worker stop --project-root "$REPO" >/dev/null 2>&1 || true; fi
  rm -rf "$WORK"; mkdir -p "$WORK"
  cargo build -q -p eaeh-cli
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
mode = "external_ai"
[execution.coordinator]
auto_start = true
provider = "claude_code"
task_timeout_secs = 2400
max_attempts = 3
notify = true
[providers.replay]
enabled = true
cassette = "$ROOT/examples/simple-springboot-api/replay-cassette.json"
EOF
  mkdir -p "$REPO"
  cp "$ROOT/examples/simple-springboot-api/BRD-Simple-Spring-Boot-API.md" "$ROOT/examples/simple-springboot-api/PROJECT-BOOTSTRAP.md" "$REPO/"
  git -C "$REPO" init -q
  cd "$REPO"
  step "0. platform setup (administrator, once)"
  "$B" init > /dev/null
  "$B" admin bootstrap --tenant acme --project platform --admin-id admin --token-file "$WORK/admin.token" > /dev/null
  "$B" auth login --profile admin --token-file "$WORK/admin.token"; rm "$WORK/admin.token"
  step "1. project bootstrap (one command)"
  set +e
  "$B" project init --name simple-springboot-api \
    --description "Simple REST API for local development and EAEH lifecycle demonstration" \
    --language "Java 21" --framework "Spring Boot 3.x" --build-tool Maven \
    --constraint "Simple layered architecture: controllers, services, models" \
    --constraint "Products in memory; no database" \
    --constraint "No authentication, authorization or Spring Security" \
    --constraint "Local development only" \
    --brd BRD-Simple-Spring-Boot-API.md --knowledge PROJECT-BOOTSTRAP.md \
    --developer dev --steward steward --approver approver \
    --mode external_ai --client claude-code \
    --build-verification deferred --store-profiles
  echo "(exit $?: 7 = not ready until the approver has reviewed the skills)"
  set -e
  step "2. automated execution: mode, provider, Claude Code sign-in"
  "$B" worker status
  echo
  echo "Next (approver, a person): scripts/demo-automated-ai.sh approve-skills"
  ;;
approve-skills)
  cd "$REPO"
  "$B" approval review --as approver
  "$B" approval review --as approver --all --yes
  "$B" project readiness
  ;;
start)
  cd "$REPO"
  step "3. developer starts BRD → requirements → HLD → LLD (the worker executes the AI tasks)"
  "$B" workflow run design --yes
  ;;
status)
  cd "$REPO"
  "$B" workflow status; echo; "$B" worker status; echo; "$B" worker history
  ;;
watch)
  cd "$REPO"; "$B" workflow watch ;;
review)
  cd "$REPO"
  "$B" review --as approver --no-show
  echo "Read the files above (in $REPO/review/<gate>/), then: scripts/demo-automated-ai.sh decide approve \"<comments>\""
  ;;
decide)
  cd "$REPO"
  "$B" review --as approver --no-show --decision "${2:?approve|request_changes|reject}" --comments "${3:-}"
  ;;
stop)
  cd "$REPO"; "$B" worker stop ;;
report)
  cd "$REPO"
  step "workflow"
  "$B" workflow status
  step "automated executions (provider, attempt, outcome, duration, auth)"
  "$B" worker history
  step "approvals (who decided what)"
  "$B" --json approval list | python3 -c "
import json,sys
for a in json.load(sys.stdin)['items']:
    print('   %-22s %-18s decided %s' % (a['gate'] or a['subject_type'], a['status'], a['decided_at']))"
  step "hand-offs (client provenance)"
  "$B" --json task list --all | python3 -c "
import json,sys
for t in json.load(sys.stdin)['tasks']:
    print('   %-8s %-30s %-9s submissions=%d client=%s dispatch=%s' % (t['state'], t['agent'], t['status'], t['submissions'], t['client'], t['dispatch_state']))"
  step "audit and model usage"
  "$B" audit verify | python3 -c "import json,sys; d=json.load(sys.stdin); print('   audit chain valid=%s records=%s' % (d['valid'], d['records_checked']))"
  python3 - "$WORK/data/eaeh.db" <<'PY'
import sqlite3,sys
db=sqlite3.connect(f"file:{sys.argv[1]}?mode=ro",uri=True)
q=lambda s: db.execute(s).fetchone()[0]
print("   EAEH-managed model usage rows:      ", q("SELECT COUNT(*) FROM model_usage"))
print("   model.invoke audit records:         ", q("SELECT COUNT(*) FROM audit_log WHERE action='model.invoke'"))
print("   external_execution.finish records:  ", q("SELECT COUNT(*) FROM audit_log WHERE action='external_execution.finish'"))
print("   approval decisions by the worker:   ", q("SELECT COUNT(*) FROM audit_log a JOIN external_workers w ON w.principal_id = a.actor_id WHERE a.action LIKE 'approval.decide%'"))
for r in db.execute("SELECT json_extract(details,'$.auth_source'), COUNT(*) FROM audit_log WHERE action='external_execution.finish' GROUP BY 1"):
    print("   Claude Code credential source (init event):", r[0], "x", r[1])
for r in db.execute("SELECT DISTINCT auth_status, client_version FROM external_executions"):
    print("   auth status / Claude Code version:  ", r[0], "/", r[1])
PY
  step "worker log (tail)"
  "$B" worker log --lines 40
  ;;
*)
  sed -n '2,20p' "$0"; exit 2 ;;
esac
