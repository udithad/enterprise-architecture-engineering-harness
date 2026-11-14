#!/usr/bin/env bash
# EAEH Phase 2 demonstration against the real `eaeh serve` binary.
#
#   scripts/demo-knowledge.sh                 # governance + knowledge + skill flow, no model calls
#   EAEH_DEMO_LIVE=1 EAEH_SECRET_ANTHROPIC_API_KEY=... scripts/demo-knowledge.sh
#                                             # additionally lets the api-reviewer agent call Claude
#                                             # (billed; only run with explicit authorisation)
#
# Requires curl and python3; uses port 18081 and a throw-away data dir under target/.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"; WORK="$ROOT/target/demo-knowledge"; rm -rf "$WORK"; mkdir -p "$WORK"
cargo build -q -p eaeh-cli
BIN="$ROOT/target/debug/eaeh"
LIVE=false
if [[ "${EAEH_DEMO_LIVE:-0}" == "1" && -n "${EAEH_SECRET_ANTHROPIC_API_KEY:-}" ]]; then LIVE=true; fi
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
bind = "127.0.0.1:18081"
[log]
format = "json"
filter = "warn"
[providers.anthropic]
enabled = $LIVE
EOF
CFG="$WORK/eaeh.toml"; U="http://127.0.0.1:18081"
j() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
post() { curl -s -X POST "$U$1" -H "Authorization: Bearer $2" -H 'content-type: application/json' -d "$3"; }

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

echo "== 1. steward registers sources and ingests the demonstration corpus"
post /v1/knowledge/sources "$S" '{"name":"demo-api-standards","kind":"standard","description":"Demonstration API standards"}' > /dev/null
post /v1/knowledge/sources "$S" '{"name":"demo-api-guides","kind":"reference","description":"Demonstration API guides"}' > /dev/null
post /v1/knowledge/sources "$S" '{"name":"demo-api-specs","kind":"api_spec","description":"Specs under review"}' > /dev/null
ingest() { # source subject file format scope mandatory
  python3 - "$@" <<PY > "$WORK/body.json"
import json,sys
src,subj,path,fmt,scope,mand=sys.argv[1:7]
sc={"level":"enterprise"} if scope=="enterprise" else {"level":"project","ref":"$P"}
print(json.dumps({"source":src,"subject":subj,"title":subj,"format":fmt,"content":open(path).read(),"scope":sc,"mandatory":mand=="true","governance_project":"$P"}))
PY
  curl -s -X POST "$U/v1/knowledge/documents" -H "Authorization: Bearer $S" -H 'content-type: application/json' --data-binary @"$WORK/body.json"
}
STD=$(ingest demo-api-standards error-format examples/knowledge/standards/error-format.md markdown enterprise true)
echo "$STD" | j "'   standard: status=' + d['status']"
ingest demo-api-guides resource-naming examples/knowledge/guides/resource-naming.md markdown enterprise false | j "'   guide: status=' + d['status']"
ingest demo-api-guides pagination examples/knowledge/guides/pagination.md markdown enterprise false | j "'   guide: status=' + d['status']"
ingest demo-api-specs orders-api examples/knowledge/specs/orders-api.json openapi project false | j "'   spec: status=' + d['status'] + ' chunks=' + str(d['chunks'])"

echo "== 2. steward cannot approve their own standard; approver approves (hash-bound)"
AID=$(echo "$STD" | j "d['approval_id']"); H=$(echo "$STD" | j "d['content_hash']")
BODY="{\"decision\":\"approve\",\"artifact_hash\":\"$H\"}"
echo "   steward approve -> $(curl -s -o /dev/null -w '%{http_code}' -X POST "$U/v1/approvals/$AID/decision" -H "Authorization: Bearer $S" -H 'content-type: application/json' -d "$BODY")"
echo "   approver approve -> $(curl -s -o /dev/null -w '%{http_code}' -X POST "$U/v1/approvals/$AID/decision" -H "Authorization: Bearer $AP" -H 'content-type: application/json' -d "$BODY")"

echo "== 3. steward registers the api-design-review skill; approver approves"
python3 -c "import json; print(json.dumps({'project_id':'$P','definition':json.load(open('config/skills/api-design-review.json'))}))" > "$WORK/skill.json"
SK=$(curl -s -X POST "$U/v1/skills" -H "Authorization: Bearer $S" -H 'content-type: application/json' --data-binary @"$WORK/skill.json")
SAID=$(echo "$SK" | j "d['approval_id']"); SD=$(echo "$SK" | j "d['digest']")
post "/v1/approvals/$SAID/decision" "$AP" "{\"decision\":\"approve\",\"artifact_hash\":\"$SD\"}" | j "'   skill approval: ' + d['status']"
curl -s "$U/v1/skills" -H "Authorization: Bearer $OP" | j "'   skills: ' + ', '.join(s['definition']['id']+'@'+s['definition']['version']+'='+s['status'] for s in d['items'])"

echo "== 4. cited, scoped search as the operator"
post /v1/knowledge/search "$OP" "{\"project_id\":\"$P\",\"text\":\"plural nouns for collections\",\"k\":3}" | j "'\\n'.join('   ' + h['citation'] + '  score=' + str(round(h['score'],5)) for h in d['items'])"

echo "== 5. operator starts knowledge.review (agent + approved skill + enterprise knowledge)"
WF=$(post /v1/workflows "$OP" "{\"definition\":\"knowledge.review\",\"project_id\":\"$P\",\"input\":{\"spec\":\"demo-api-specs/orders-api\"}}" | j "d['id']")
for _ in $(seq 1 240); do
  ST=$(curl -s "$U/v1/workflows/$WF" -H "Authorization: Bearer $OP" | j "d['instance']['status']")
  [[ "$ST" != "running" ]] && break; sleep 0.5
done
INST=$(curl -s "$U/v1/workflows/$WF" -H "Authorization: Bearer $OP")
echo "   workflow status: $ST"
if [[ "$ST" == "waiting_approval" ]]; then
  echo "$INST" | j "'   review artifact: ' + d['instance']['data']['artifacts']['review']['name'] + ' v' + str(d['instance']['data']['artifacts']['review']['version'])"
  echo "$INST" | j "'   findings: ' + json.dumps(d['instance']['data']['outputs']['review'].get('findings', []))[:600]" 2>/dev/null || true
elif [[ "$LIVE" == false ]]; then
  echo "$INST" | j "'   reason: ' + str(d['instance']['data'].get('failure'))"
  echo "   NOTE: no model provider is enabled (live calls not authorised). The agent step is"
  echo "         verified deterministically in tests/tests/knowledge_demo.rs. To run it live:"
  echo "         EAEH_DEMO_LIVE=1 EAEH_SECRET_ANTHROPIC_API_KEY=... scripts/demo-knowledge.sh"
fi
echo "== 6. audit chain"; curl -s "$U/v1/audit/verify" -H "Authorization: Bearer $A" | j "'   valid=' + str(d['valid']) + ' records=' + str(d['records_checked'])"
echo "== key/token literals in server log: $(grep -cE 'eaeh_[0-9a-f]{64}|sk-ant-' "$WORK/server.log" || true)"
echo "== DEMO COMPLETE (live model: $LIVE)"
