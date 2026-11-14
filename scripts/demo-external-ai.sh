#!/usr/bin/env bash
# EAEH external-AI acceptance scenario (ADR-0025) on `simple-springboot-api`.
#
# EAEH runs in `external_ai` mode. It makes no model calls. The AI work (BRD analysis,
# requirements, HLD, LLD, implementation plan, code, tests) is done by the connected AI
# client (Claude Code, GitHub Copilot) through the hand-off commands, using the
# developer's own subscription. This script does the platform and human-approver parts;
# the client does the rest.
#
#   scripts/demo-external-ai.sh setup             # platform, bootstrap, approvals, start `design`
#   source <(scripts/demo-external-ai.sh env)     # shell environment for the hand-off commands
#   eaeh task list / task show / task tool / task submit   # done by the AI client
#   scripts/demo-external-ai.sh approve "<comment>" [request_changes|approve]
#                                                 # the human approver decides the pending gate
#   scripts/demo-external-ai.sh deliver           # start the governed delivery (after an approved design)
#   scripts/demo-external-ai.sh export            # export the approved source bundle
#   scripts/demo-external-ai.sh report            # step 13: artifacts, traces, approvals, audit, model usage
#
# Works on a throw-away copy under target/demo-external-ai; your repositories are not
# modified. Build and test verification is `deferred` here, because this workstation has
# no isolated JDK 21 + Maven toolchain. The delivered bundle says so, and readiness warns.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"; WORK="$ROOT/target/demo-external-ai"; REPO="$WORK/simple-springboot-api"
B="$ROOT/target/debug/eaeh"
export EAEH_HOME="$WORK/home" EAEH_CONFIG="$WORK/eaeh.toml"
unset EAEH_TOKEN EAEH_PROFILE || true
j() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
step() { echo; echo "== $*"; }

case "${1:-}" in
env)
  echo "export EAEH_HOME='$EAEH_HOME' EAEH_CONFIG='$EAEH_CONFIG' PATH='$ROOT/target/debug':\"\$PATH\"; cd '$REPO'"
  ;;
setup)
  rm -rf "$WORK"; mkdir -p "$WORK"
  cargo build -q -p eaeh-cli
  # Providers are enabled deliberately: external_ai mode must ignore them (no fallback).
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
# This script demonstrates the interactive / manual hand-off path (ADR-0025); the
# automated worker (ADR-0026) is shown by scripts/demo-automated-ai.sh.
[execution.coordinator]
auto_start = false
[providers.replay]
enabled = true
cassette = "$ROOT/examples/simple-springboot-api/replay-cassette.json"
EOF
  mkdir -p "$REPO"
  cp "$ROOT/examples/simple-springboot-api/BRD-Simple-Spring-Boot-API.md" "$ROOT/examples/simple-springboot-api/PROJECT-BOOTSTRAP.md" "$REPO/"
  if command -v git >/dev/null; then git -C "$REPO" init -q; fi
  cd "$REPO"
  step "0. one-time platform setup (administrator)"
  "$B" init > /dev/null
  "$B" admin bootstrap --tenant acme --project platform --admin-id admin --token-file "$WORK/admin.token" > /dev/null
  "$B" auth login --profile admin --token-file "$WORK/admin.token"; rm "$WORK/admin.token"
  step "1. bootstrap the project (one command; external_ai; Claude Code + GitHub Copilot)"
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
    --mode external_ai --client claude-code --client github-copilot \
    --build-verification deferred --store-profiles
  echo "(exit $?: 7 = not ready yet)"
  set -e
  echo "   generated: $(cd "$REPO" && find . -path ./.git -prune -o -type f -print | sort | tr '\n' ' ')"
  step "2. readiness (before approvals)"
  set +e; "$B" project readiness; echo "(exit $?)"; set -e
  step "3. required skills reviewed and approved by the human approver"
  "$B" skill pending
  "$B" approval review --as approver --all --yes
  step "2b. readiness (after approvals)"
  "$B" project readiness
  step "4. BRD identified"
  grep -A2 '^\[brd\]' .eaeh/project.toml
  step "5–8. start BRD → requirements → HLD → LLD (plan, then run); agent stages are handed to the AI client"
  "$B" workflow run design
  "$B" workflow run design --yes
  ;;
approve)
  cd "$REPO"
  gate() { "$B" --json workflow status | j "$1"; }
  P=$("$B" --json approval list --status pending)
  AID=$(echo "$P" | j "[a for a in d['items'] if a['subject_type']=='workflow_gate'][0]['id']")
  HASH=$(echo "$P" | j "[a for a in d['items'] if a['subject_type']=='workflow_gate'][0]['artifact_hash']")
  ART=$(echo "$P" | j "[a for a in d['items'] if a['subject_type']=='workflow_gate'][0]['artifact_id']")
  STAGE=$(gate "d['stage']")
  "$B" artifact export "$ART" --out "$WORK/review/$STAGE" 2>/dev/null | j "'   exported for review: ' + ', '.join(d['written'])"
  "$B" approval decide "$AID" --decision "${3:-approve}" --comments "$2" --artifact-hash "$HASH" --as approver \
    | j "'   %s: %s -> workflow %s' % ('$STAGE', d['approval']['status'], d['workflow_status'])"
  "$B" workflow status | tail -4
  ;;
deliver)
  cd "$REPO"
  step "9. implementation plan, implementation and tests (governed delivery; plan first)"
  "$B" workflow run deliver
  "$B" workflow run deliver --yes
  ;;
export)
  cd "$REPO"
  ID=$("$B" --json workflow status | j "[a['id'] for a in d['artifacts'] if a['state']=='snapshot'][0]")
  "$B" artifact export "$ID" --out "$WORK/delivered" | j "'   approved=%s files=%d' % (bool(d['approved']), len(d['written']))"
  find "$WORK/delivered" -type f | sed "s#$WORK/##" | sort
  ;;
report)
  cd "$REPO"
  step "13. artifacts, traceability, approvals, hand-offs, audit and model usage"
  { find "$WORK/review" "$WORK/delivered" -type f 2>/dev/null || true; } | sed "s#$WORK/##" | sort
  "$B" trace --id REQ-F-003 --depth 2 | j "'\\n'.join('   %s %s -[%s]-> %s %s' % (l['from_kind'], l['from_id'], l['relation'], l['to_kind'], l['to_id']) for l in d['items'])"
  "$B" --json approval list | j "'\\n'.join('   %-22s %-18s decided %s' % (a['gate'] or a['subject_type'], a['status'], a['decided_at']) for a in d['items'])"
  "$B" --json task list --all | j "'\\n'.join('   hand-off %-26s %-28s %-9s submissions=%d client=%s' % (t['task_kind'], t['agent'], t['status'], t['submissions'], t['client']) for t in d['tasks'])"
  "$B" audit list --limit 1000 | j "'   audit records: %d; actions: %s' % (len(d['items']), ', '.join(sorted(set(r['action'] for r in d['items']))))"
  "$B" audit verify | j "'   audit chain valid=' + str(d['valid']) + ' records=' + str(d['records_checked'])"
  python3 - "$WORK/data/eaeh.db" <<'PY'
import sqlite3,sys
db=sqlite3.connect(f"file:{sys.argv[1]}?mode=ro",uri=True)
q=lambda s: db.execute(s).fetchone()[0]
print("   EAEH-managed model usage rows:", q("SELECT COUNT(*) FROM model_usage"))
print("   model.invoke audit records:   ", q("SELECT COUNT(*) FROM audit_log WHERE action='model.invoke'"))
print("   model invoked events:         ", q("SELECT COUNT(*) FROM outbox WHERE event_type LIKE 'model.%'"))
PY
  echo "   secrets in generated files: $(grep -c 'eaeh_[0-9a-f]\{64\}' CLAUDE.md .mcp.json .vscode/mcp.json .github/copilot-instructions.md .eaeh/project.toml | paste -sd' ' -)"
  ;;
*)
  sed -n '2,22p' "$0"; exit 2 ;;
esac
