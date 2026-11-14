#!/usr/bin/env bash
# EAEH API smoke test against the real `eaeh serve` binary (no test doubles).
# Usage: scripts/smoke-api.sh            (requires curl, python3; port 18080 free)
# Creates a throw-away data directory under target/smoke and removes it afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
WORK="$ROOT/target/smoke"
rm -rf "$WORK" && mkdir -p "$WORK"
cargo build -q -p eaeh-cli
BIN="$ROOT/target/debug/eaeh"
cat > "$WORK/eaeh.toml" <<EOF
[storage]
data_dir = "$WORK/data"
[definitions]
prompts_dir = "$ROOT/prompts"
agents_dir = "$ROOT/config/agents"
workflows_dir = "$ROOT/workflows"
policies_dir = "$ROOT/config/policies"
[isolation]
container_enabled = true
exec_allowed_programs = ["/bin/echo"]
[api]
bind = "127.0.0.1:18080"
[log]
format = "json"
filter = "info,sqlx=warn"
EOF
CFG="$WORK/eaeh.toml"
U="http://127.0.0.1:18080"
j() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }

echo "== init";      "$BIN" --config "$CFG" init | j "d['health']['status'], d['health']['migrations']"
echo "== bootstrap"; "$BIN" --config "$CFG" admin bootstrap --tenant acme --project platform --admin-id admin --token-file "$WORK/admin.token" > "$WORK/boot.json"
echo "token file mode: $(stat -f '%Lp' "$WORK/admin.token" 2>/dev/null || stat -c '%a' "$WORK/admin.token")"
A=$(cat "$WORK/admin.token"); P=$(j "d['project_id']" < "$WORK/boot.json")

"$BIN" --config "$CFG" serve > "$WORK/server.log" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do curl -sf -o /dev/null "$U/v1/health" && break; sleep 0.25; done

echo "== health (unauthenticated)"; curl -s "$U/v1/health"; echo
echo "== /v1/me without token -> $(curl -s -o /dev/null -w '%{http_code}' "$U/v1/me")"
for who in "olivia operator" "arthur approver"; do
  set -- $who
  curl -sf -X POST "$U/v1/principals" -H "Authorization: Bearer $A" -H 'content-type: application/json' \
    -d "{\"id\":\"$1\",\"display_name\":\"$1\",\"roles\":[\"$2\"],\"projects\":[\"$P\"]}" > /dev/null
done
OP=$(curl -s -X POST "$U/v1/tokens" -H "Authorization: Bearer $A" -H 'content-type: application/json' -d '{"principal_id":"olivia","label":"smoke"}' | j "d['token']")
AP=$(curl -s -X POST "$U/v1/tokens" -H "Authorization: Bearer $A" -H 'content-type: application/json' -d '{"principal_id":"arthur","label":"smoke"}' | j "d['token']")

echo "== start demo.approval as operator"
WF=$(curl -s -X POST "$U/v1/workflows" -H "Authorization: Bearer $OP" -H 'content-type: application/json' -d "{\"definition\":\"demo.approval\",\"project_id\":\"$P\"}" | j "d['id']")
sleep 1
curl -s "$U/v1/workflows/$WF" -H "Authorization: Bearer $OP" | j "'status=' + d['instance']['status'] + ' state=' + d['instance']['state']"
APR=$(curl -s "$U/v1/approvals?project_id=$P&status=pending" -H "Authorization: Bearer $AP")
AID=$(echo "$APR" | j "d['items'][0]['id']"); H=$(echo "$APR" | j "d['items'][0]['artifact_hash']")
BODY="{\"decision\":\"approve\",\"artifact_hash\":\"$H\"}"
CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$U/v1/approvals/$AID/decision" -H "Authorization: Bearer $OP" -H 'content-type: application/json' -d "$BODY")
echo "== operator tries to approve -> $CODE"
echo "== approver with wrong artifact hash -> $(curl -s -o /dev/null -w '%{http_code}' -X POST "$U/v1/approvals/$AID/decision" -H "Authorization: Bearer $AP" -H 'content-type: application/json' -d '{"decision":"approve","artifact_hash":"0000"}')"
echo "== approver approves"
curl -s -X POST "$U/v1/approvals/$AID/decision" -H "Authorization: Bearer $AP" -H 'content-type: application/json' \
  -d "{\"decision\":\"approve\",\"comments\":\"smoke ok\",\"artifact_hash\":\"$H\"}" | j "'approval=' + d['status'] + ' by ' + d['decision']['approver_id']"
sleep 1
curl -s "$U/v1/workflows/$WF" -H "Authorization: Bearer $OP" | j "'status=' + d['instance']['status'] + ' path=' + ' '.join(h['from_state']+'->'+h['to_state'] for h in d['history'])"
echo "== audit verify (admin)"; curl -s "$U/v1/audit/verify" -H "Authorization: Bearer $A" | j "'valid=' + str(d['valid']) + ' records=' + str(d['records_checked'])"
echo "== audit as operator -> $(curl -s -o /dev/null -w '%{http_code}' "$U/v1/audit" -H "Authorization: Bearer $OP")"
echo "== metrics"; curl -s "$U/v1/metrics" -H "Authorization: Bearer $A" | grep -E 'workflow.completed|outbox_pending' || true
echo "== isolation providers"; curl -s "$U/v1/health/details" -H "Authorization: Bearer $A" | j "d['isolation']"
echo "== token literals in server log: $(grep -cE 'eaeh_[0-9a-f]{64}' "$WORK/server.log" || true)"
kill $PID; wait $PID 2>/dev/null || true
trap - EXIT
rm -rf "$WORK"
echo "== SMOKE PASSED"
