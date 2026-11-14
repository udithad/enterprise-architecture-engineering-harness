#!/usr/bin/env bash
# EAEH Phase 3 demonstration against the real `eaeh serve` binary:
# governed BRD → requirements → HLD → LLD, each with a deterministic quality gate and a
# human approval gate (one request-changes round trip on the requirements).
#
#   scripts/demo-design.sh             # governance + BRD + skills + workflow start; no model calls
#   EAEH_DEMO_LIVE=1 EAEH_ANTHROPIC_KEY_FILE=~/.eaeh/anthropic.key scripts/demo-design.sh
#   EAEH_DEMO_LIVE=1 EAEH_SECRET_ANTHROPIC_API_KEY=... scripts/demo-design.sh
#                                      # live run with Claude (billed; authorised as P3-D1).
#                                      # The key is never printed; prefer the key file (0600).
#
# Cost cap: each agent run is limited to $5 and the workflow to $25 by budgets in the
# shipped definitions (enforced by the budget ledger, fail closed).
# Requires curl and python3; uses port 18082 and a throw-away data dir under target/.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"; WORK="$ROOT/target/demo-design"; rm -rf "$WORK"; mkdir -p "$WORK"
cargo build -q -p eaeh-cli
BIN="$ROOT/target/debug/eaeh"
if [[ -n "${EAEH_ANTHROPIC_KEY_FILE:-}" && -z "${EAEH_SECRET_ANTHROPIC_API_KEY:-}" ]]; then
  EAEH_SECRET_ANTHROPIC_API_KEY="$(tr -d '[:space:]' < "${EAEH_ANTHROPIC_KEY_FILE/#\~/$HOME}")"
  export EAEH_SECRET_ANTHROPIC_API_KEY
fi
LIVE=false
if [[ "${EAEH_DEMO_LIVE:-0}" == "1" && -n "${EAEH_SECRET_ANTHROPIC_API_KEY:-}" ]]; then LIVE=true; fi
if [[ "$LIVE" == false ]]; then unset EAEH_SECRET_ANTHROPIC_API_KEY || true; fi
cat > "$WORK/eaeh.toml" <<EOF
[storage]
data_dir = "$WORK/data"
[definitions]
prompts_dir = "$ROOT/prompts"
agents_dir = "$ROOT/config/agents"
workflows_dir = "$ROOT/workflows"
policies_dir = "$ROOT/config/policies"
[isolation]
container_enabled = false
[api]
bind = "127.0.0.1:18082"
[log]
format = "json"
filter = "warn"
[providers.anthropic]
enabled = $LIVE
EOF
CFG="$WORK/eaeh.toml"; U="http://127.0.0.1:18082"
j() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
post() { curl -s -X POST "$U$1" -H "Authorization: Bearer $2" -H 'content-type: application/json' -d "$3"; }
postf() { curl -s -X POST "$U$1" -H "Authorization: Bearer $2" -H 'content-type: application/json' --data-binary @"$3"; }

"$BIN" --config "$CFG" admin bootstrap --tenant acme --project orders --admin-id admin --token-file "$WORK/admin.token" > "$WORK/boot.json"
A=$(cat "$WORK/admin.token"); P=$(j "d['project_id']" < "$WORK/boot.json")
"$BIN" --config "$CFG" serve > "$WORK/server.log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do curl -sf -o /dev/null "$U/v1/health" && break; sleep 0.25; done

for who in "sam knowledge_steward" "arthur approver" "olivia operator"; do
  set -- $who
  post /v1/principals "$A" "{\"id\":\"$1\",\"display_name\":\"$1\",\"roles\":[\"$2\"]}" > /dev/null
done
tok() { post /v1/tokens "$A" "{\"principal_id\":\"$1\",\"label\":\"demo\"}" | j "d['token']"; }
S=$(tok sam); AP=$(tok arthur); OP=$(tok olivia)

echo "== 1. steward ingests the demonstration standard (approved) and the BRD (project-scoped, confidential)"
post /v1/knowledge/sources "$S" '{"name":"demo-api-standards","kind":"standard","description":"Demonstration API standards"}' > /dev/null
post /v1/knowledge/sources "$S" '{"name":"demo-brd","kind":"reference","description":"Demonstration BRDs"}' > /dev/null
doc() { # source subject path scope mandatory classification tags
  python3 - "$@" <<PY > "$WORK/body.json"
import json,sys
src,subj,path,scope,mand,cls,tags=sys.argv[1:8]
sc={"level":"enterprise"} if scope=="enterprise" else {"level":"project","ref":"$P"}
print(json.dumps({"source":src,"subject":subj,"title":subj,"format":"markdown","content":open(path).read(),"scope":sc,
  "mandatory":mand=="true","classification":cls,"tags":[t for t in tags.split(",") if t],"governance_project":"$P"}))
PY
  postf /v1/knowledge/documents "$S" "$WORK/body.json"
}
STD=$(doc demo-api-standards error-format examples/knowledge/standards/error-format.md enterprise true internal "")
post "/v1/approvals/$(echo "$STD" | j "d['approval_id']")/decision" "$AP" "{\"decision\":\"approve\",\"artifact_hash\":\"$(echo "$STD" | j "d['content_hash']")\"}" | j "'   standard: ' + d['status']"
doc demo-brd order-management examples/brd/order-management.md project false confidential brd | j "'   BRD: status=' + d['status'] + ' chunks=' + str(d['chunks']) + ' classification=' + d['classification']"

echo "== 2. steward registers the Phase 3 skills; approver approves each (digest-bound)"
for s in requirements-analysis hld-design lld-design; do
  python3 -c "import json; print(json.dumps({'project_id':'$P','definition':json.load(open('config/skills/$s.json'))}))" > "$WORK/skill.json"
  SK=$(postf /v1/skills "$S" "$WORK/skill.json")
  post "/v1/approvals/$(echo "$SK" | j "d['approval_id']")/decision" "$AP" "{\"decision\":\"approve\",\"artifact_hash\":\"$(echo "$SK" | j "d['digest']")\"}" | j "'   $s: ' + d['status']"
done

echo "== 3. operator starts engineering.design"
WF=$(post /v1/workflows "$OP" "{\"definition\":\"engineering.design\",\"project_id\":\"$P\",\"input\":{\"brd\":\"demo-brd/order-management@v1\"}}" | j "d['id']")
REVISED=false
show_bundle() { # state
  curl -s "$U/v1/workflows/$WF" -H "Authorization: Bearer $OP" > "$WORK/inst.json"
  python3 - "$1" "$WORK" "$U" "$OP" "$P" <<'PY'
import json,sys,urllib.request,os
state,work,u,tok,p=sys.argv[1:6]
inst=json.load(open(os.path.join(work,"inst.json")))["instance"]
ref=inst["data"]["artifacts"][state]
def get(aid):
    r=urllib.request.Request(f"{u}/v1/artifacts/{aid}?project_id={p}",headers={"Authorization":f"Bearer {tok}"})
    return json.load(urllib.request.urlopen(r))
m=json.loads(get(ref["id"])["content"])
out=os.path.join(work,"export",ref["name"])
for path,f in sorted(m["files"].items()):
    a=get(f["artifact_id"]); assert a["artifact"]["content_hash"]==f["hash"]
    t=os.path.join(out,path); os.makedirs(os.path.dirname(t),exist_ok=True); open(t,"w").write(a["content"])
print(f"   bundle {ref['name']} v{ref['version']} ({len(m['files'])} files, manifest {ref['hash'][:12]}…) → {out}")
for k,v in sorted(inst["data"].get("quality",{}).items()): print(f"   quality gate {k}: {v}")
PY
}
for _ in $(seq 1 12); do
  for _ in $(seq 1 1800); do
    ST=$(curl -s "$U/v1/workflows/$WF" -H "Authorization: Bearer $OP" | j "d['instance']['status']")
    [[ "$ST" != "running" ]] && break; sleep 1
  done
  INST=$(curl -s "$U/v1/workflows/$WF" -H "Authorization: Bearer $OP")
  STATE=$(echo "$INST" | j "d['instance']['state']")
  echo "   workflow: status=$ST state=$STATE"
  [[ "$ST" != "waiting_approval" ]] && break
  FROM=$(echo "$INST" | j "{'approve_requirements':'render_requirements','approve_hld':'render_hld','approve_lld':'render_lld'}[d['instance']['state']]")
  show_bundle "$FROM"
  GATE=$(curl -s "$U/v1/approvals?project_id=$P&status=pending" -H "Authorization: Bearer $AP")
  GID=$(echo "$GATE" | j "d['items'][0]['id']"); GH=$(echo "$GATE" | j "d['items'][0]['artifact_hash']")
  echo "   operator tries to approve -> $(curl -s -o /dev/null -w '%{http_code}' -X POST "$U/v1/approvals/$GID/decision" -H "Authorization: Bearer $OP" -H 'content-type: application/json' -d "{\"decision\":\"approve\",\"artifact_hash\":\"$GH\"}")"
  if [[ "$STATE" == "approve_requirements" && "$REVISED" == false ]]; then
    REVISED=true
    post "/v1/approvals/$GID/decision" "$AP" "{\"decision\":\"request_changes\",\"comments\":\"State the rationale for every non-functional target.\",\"artifact_hash\":\"$GH\"}" | j "'   approver: request_changes -> ' + d['status']"
  else
    post "/v1/approvals/$GID/decision" "$AP" "{\"decision\":\"approve\",\"comments\":\"approved in demo\",\"artifact_hash\":\"$GH\"}" | j "'   approver: approve ' + d['gate'] + ' -> ' + d['status']"
  fi
done
echo "$INST" | j "'   final: ' + d['instance']['status'] + (' – ' + str(d['instance']['data'].get('failure')) if d['instance']['data'].get('failure') else '')"
if [[ "$ST" == "failed" && "$LIVE" == false ]]; then
  echo "   NOTE: no model provider is enabled in this run (no API key supplied). The agent steps"
  echo "         are verified deterministically in tests/tests/design_demo.rs. To run live:"
  echo "         EAEH_DEMO_LIVE=1 EAEH_ANTHROPIC_KEY_FILE=~/.eaeh/anthropic.key scripts/demo-design.sh"
fi

echo "== 4. traceability from the BRD 'Payments' section"
curl -s "$U/v1/traceability?project_id=$P&id=demo-brd/order-management@v1%235&depth=3" -H "Authorization: Bearer $OP" \
  | j "'\\n'.join('   %s %s -[%s]-> %s %s' % (l['from_kind'], l['from_id'], l['relation'], l['to_kind'], l['to_id']) for l in d['items'][:25]) or '   (no links – nothing approved)'"

echo "== 5. model usage (from the governed usage records)"
python3 - "$WORK/data/eaeh.db" <<'PY'
import sqlite3,sys,json
db=sqlite3.connect(f"file:{sys.argv[1]}?mode=ro",uri=True)
rows=db.execute("SELECT provider_id, model_id, model_version, COUNT(*), SUM(input_tokens), SUM(output_tokens), SUM(cost_micros) FROM model_usage GROUP BY 1,2,3").fetchall()
if not rows: print("   no model calls")
for r in rows: print(f"   {r[0]}/{r[1]} (reported version {r[2]}): calls={r[3]} input_tokens={r[4]} output_tokens={r[5]} cost=${(r[6] or 0)/1e6:.4f}")
den=db.execute("SELECT COUNT(*) FROM audit_log WHERE action='model.invoke' AND outcome='denied'").fetchone()[0]
print(f"   egress denials: {den}")
PY
echo "== 6. audit chain"; curl -s "$U/v1/audit/verify" -H "Authorization: Bearer $A" | j "'   valid=' + str(d['valid']) + ' records=' + str(d['records_checked'])"
echo "== key/token literals in server log: $(grep -cE 'eaeh_[0-9a-f]{64}|sk-ant-' "$WORK/server.log" || true)"
echo "== DEMO COMPLETE (live model: $LIVE)"
