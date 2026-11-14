#!/usr/bin/env bash
# EAEH developer-experience regression scenario: onboarding `simple-springboot-api` and
# running BRD → requirements → HLD → LLD with the CLI only, in `deterministic` execution
# mode (recorded, hand-authored responses – NOT AI; no AI subscription or API key needed).
# The AI-driven scenario (Claude Code / Copilot doing the reasoning) is
# scripts/demo-external-ai.sh.
#
#   scripts/demo-onboarding.sh
#
# Works on a throw-away copy under target/demo-onboarding (your repositories are not
# modified). Requires python3 for JSON extraction; uses git if available.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"; WORK="$ROOT/target/demo-onboarding"; rm -rf "$WORK"; mkdir -p "$WORK"
cargo build -q -p eaeh-cli
B="$ROOT/target/debug/eaeh"
export EAEH_HOME="$WORK/home" EAEH_CONFIG="$WORK/eaeh.toml"
unset EAEH_TOKEN EAEH_PROFILE || true
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
REPO="$WORK/simple-springboot-api"; mkdir -p "$REPO"
cp "$ROOT/examples/simple-springboot-api/BRD-Simple-Spring-Boot-API.md" "$ROOT/examples/simple-springboot-api/PROJECT-BOOTSTRAP.md" "$REPO/"
if command -v git >/dev/null; then git -C "$REPO" init -q; fi
cd "$REPO"
j() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
step() { echo; echo "== $*"; }

step "0. one-time platform setup (administrator)"
"$B" init > /dev/null
"$B" admin bootstrap --tenant acme --project platform --admin-id admin --token-file "$WORK/admin.token" > /dev/null
"$B" auth login --profile admin --token-file "$WORK/admin.token"
rm "$WORK/admin.token"

step "1. register/bootstrap the project (one command)"
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
  --mode deterministic --client claude-code \
  --workflow requirements.analysis --workflow engineering.design --store-profiles
echo "(exit $?: 7 = not ready yet)"
step "1b. repeat run is idempotent"
"$B" --json project init --name simple-springboot-api --language "Java 21" --framework "Spring Boot 3.x" --build-tool Maven \
  --brd BRD-Simple-Spring-Boot-API.md --knowledge PROJECT-BOOTSTRAP.md \
  --developer dev --steward steward --approver approver --store-profiles \
  | j "'\\n'.join('   ' + a for a in d['actions'] if 'unchanged' in a or 'already' in a or 'existing' in a)"
set -e
echo "   files: $(ls -A | tr '\n' ' ')"

step "2. verify readiness"
set +e; "$B" project readiness; echo "(exit $?)"; set -e

step "3. required knowledge and skills"
"$B" auth status
"$B" skill pending

step "4. human approvals of skills (approver; each item reviewed, then confirmed)"
"$B" approval review --as approver --all --yes

step "5. BRD identified; project ready"
grep -A2 '^\[brd\]' .eaeh/project.toml
set +e; "$B" project readiness; RC=$?; set -e; echo "(exit $RC)"; [[ $RC == 0 ]]

step "6. analyse the BRD and generate traceable requirements (plan, then run)"
"$B" workflow run design
"$B" workflow run design --yes

gate() { "$B" --json workflow status | j "$1"; }
decide() { # decision comments
  local AID HASH ART
  AID=$("$B" --json approval list --status pending | j "[a for a in d['items'] if a['subject_type']=='workflow_gate'][0]['id']")
  HASH=$("$B" --json approval list --status pending | j "[a for a in d['items'] if a['subject_type']=='workflow_gate'][0]['artifact_hash']")
  ART=$("$B" --json approval list --status pending | j "[a for a in d['items'] if a['subject_type']=='workflow_gate'][0]['artifact_id']")
  "$B" artifact export "$ART" --out "$WORK/review/$(gate "d['stage']")" | j "'   exported: ' + ', '.join(d['written'])"
  "$B" approval decide "$AID" --decision "$1" --comments "$2" --artifact-hash "$HASH" --as approver \
    | j "'   decision: ' + d['approval']['status'] + ' -> workflow ' + str(d['workflow_status'])"
}

step "7. review and approve the requirements (one request-changes round first)"
decide request_changes "Derive the reported version from the Maven build"
echo "   --- excerpt of the reviewed requirements.md ---"
sed -n '1,25p' "$WORK/review/approve_requirements/requirements.md" | sed 's/^/   | /'
"$B" workflow status | head -20
decide approve "Requirements approved"

step "8. HLD generated and approved"
decide approve "HLD approved"
step "9. LLD generated and approved"
decide approve "LLD approved"
"$B" workflow status

step "10–12. implementation plan, implementation, tests"
echo "   not part of this deterministic scenario (no recorded delivery fixtures);"
echo "   governed delivery is demonstrated in external_ai mode: scripts/demo-external-ai.sh"

step "13. inspect artifacts, traces, approvals and audit"
find "$WORK/review" -type f | sed "s#$WORK/##" | sort
"$B" trace --id REQ-F-003 --depth 2 | j "'\\n'.join('   %s %s -[%s]-> %s %s' % (l['from_kind'], l['from_id'], l['relation'], l['to_kind'], l['to_id']) for l in d['items'])"
"$B" --json approval list | j "'\\n'.join('   %-22s %-18s decided %s' % (a['gate'] or a['subject_type'], a['status'], a['decided_at']) for a in d['items'])"
"$B" audit list --limit 500 | j "'   audit records: %d; actions: %s' % (len(d['items']), ', '.join(sorted(set(r['action'] for r in d['items']))))"
"$B" audit verify | j "'   audit chain valid=' + str(d['valid']) + ' records=' + str(d['records_checked'])"
python3 - "$WORK/data/eaeh.db" <<'PY'
import sqlite3,sys
db=sqlite3.connect(f"file:{sys.argv[1]}?mode=ro",uri=True)
for r in db.execute("SELECT provider_id, model_id, model_version, COUNT(*), SUM(cost_micros) FROM model_usage GROUP BY 1,2,3"):
    print(f"   model usage: {r[0]}/{r[1]} version={r[2]} calls={r[3]} cost=${(r[4] or 0)/1e6:.2f}  (deterministic replay – not AI)")
PY
echo "   secrets in generated files: $(grep -c 'eaeh_[0-9a-f]\{64\}' CLAUDE.md .mcp.json .eaeh/project.toml | paste -sd' ' -)"
echo; echo "== ONBOARDING SCENARIO COMPLETE (deterministic replay; delivery: see demo-external-ai.sh)"
