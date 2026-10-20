#!/usr/bin/env bash
# Boot the server, exercise the Phase 4 slice, then shut it down.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@127.0.0.1:5432/squid_test}"
PORT="${SMOKE_PORT:-3901}"
BASE="http://127.0.0.1:${PORT}"
LOG="$(mktemp -t squid-smoke)"

cargo build -q -p squid-server

DATABASE_URL="$DATABASE_URL" \
APP_SECRET="smoke-test-secret-at-least-32-characters-long" \
BIND_ADDR="127.0.0.1:${PORT}" \
FRONTEND_URL="http://localhost:3001" \
./target/debug/squid-server >"$LOG" 2>&1 &
SERVER_PID=$!
trap 'kill "$SERVER_PID" 2>/dev/null || true' EXIT

for _ in $(seq 1 40); do
  if curl -fsS "${BASE}/healthz" >/dev/null 2>&1; then break; fi
  sleep 0.25
done

test "$(curl -s "${BASE}/healthz")" = "ok" || { echo "health check failed"; cat "$LOG"; exit 1; }

EMAIL="smoke-$(date +%s)-$$@example.com"
SIGNUP_STATUS=$(curl -s -o /tmp/squid-smoke-signup.json -w '%{http_code}' \
  -X POST "${BASE}/auth/signup" -H 'Content-Type: application/json' \
  -d "{\"email\":\"${EMAIL}\",\"password\":\"super-secret-password\",\"workspaceName\":\"Smoke Co\"}")
test "$SIGNUP_STATUS" = "201" || { echo "signup failed: $SIGNUP_STATUS"; cat /tmp/squid-smoke-signup.json; exit 1; }

TOKEN=$(python3 -c "import json;print(json.load(open('/tmp/squid-smoke-signup.json'))['tokens']['accessOrWorkspaceAgnosticToken']['token'])")

CREATE_STATUS=$(curl -s -o /tmp/squid-smoke-create.json -w '%{http_code}' \
  -X POST "${BASE}/rest/people" -H "Authorization: Bearer ${TOKEN}" -H 'Content-Type: application/json' \
  -d '{"name":{"firstName":"Ada","lastName":"Lovelace"},"emails":{"primaryEmail":"ada@example.com"}}')
test "$CREATE_STATUS" = "201" || { echo "create failed: $CREATE_STATUS"; cat /tmp/squid-smoke-create.json; exit 1; }
python3 - <<'PY'
import json
body = json.load(open('/tmp/squid-smoke-create.json'))
assert body['data']['createPerson']['name']['firstName'] == 'Ada', body
print('create OK: createPerson envelope matches contract')
PY

PERSON_ID=$(python3 -c "import json;print(json.load(open('/tmp/squid-smoke-create.json'))['data']['createPerson']['id'])")
UPDATE_STATUS=$(curl -s -o /tmp/squid-smoke-update.json -w '%{http_code}' \
  -X PATCH "${BASE}/rest/people/${PERSON_ID}" -H "Authorization: Bearer ${TOKEN}" -H 'Content-Type: application/json' \
  -d '{"jobTitle":"Countess"}')
test "$UPDATE_STATUS" = "200" || { echo "update failed: $UPDATE_STATUS"; cat /tmp/squid-smoke-update.json; exit 1; }
python3 -c "import json;b=json.load(open('/tmp/squid-smoke-update.json'));assert b['data']['updatePerson']['jobTitle']=='Countess',b;print('update OK: updatePerson envelope matches contract')"

LIST_STATUS=$(curl -s -o /tmp/squid-smoke-list.json -w '%{http_code}' \
  "${BASE}/rest/people?limit=10" -H "Authorization: Bearer ${TOKEN}")
test "$LIST_STATUS" = "200" || { echo "list failed: $LIST_STATUS"; exit 1; }
python3 - <<'PY'
import json
body = json.load(open('/tmp/squid-smoke-list.json'))
assert body['totalCount'] == 1, body
assert body['data']['people'][0]['name']['firstName'] == 'Ada', body
assert isinstance(body['pageInfo']['startCursor'], str), body
assert isinstance(body['pageInfo']['endCursor'], str), body
assert body['pageInfo']['hasNextPage'] is False, body
print('list OK: totalCount=1, envelope and cursors match contract')
PY

METADATA_STATUS=$(curl -s -o /tmp/squid-smoke-metadata.json -w '%{http_code}' \
  -X POST "${BASE}/metadata" -H "Authorization: Bearer ${TOKEN}" -H 'Content-Type: application/json' \
  -d '{"query":"{ objects(paging:{first:100}) { edges { node { nameSingular fieldsList { name type } } } } getViews { id name type viewFields { fieldMetadataId isVisible position } } }"}')
test "$METADATA_STATUS" = "200" || { echo "metadata failed: $METADATA_STATUS"; cat /tmp/squid-smoke-metadata.json; exit 1; }
python3 - <<'PY'
import json
body = json.load(open('/tmp/squid-smoke-metadata.json'))
assert 'errors' not in body, body
objects = body['data']['objects']['edges']
assert len(objects) == 1, body
node = objects[0]['node']
assert node['nameSingular'] == 'person', body
fields = {field['name']: field['type'] for field in node['fieldsList']}
assert fields['name'] == 'FULL_NAME', fields
assert fields['emails'] == 'EMAILS', fields
views = body['data']['getViews']
assert len(views) == 1, body
assert views[0]['name'] == 'All people', body
assert views[0]['type'] == 'TABLE', body
assert len(views[0]['viewFields']) == 5, body
print('metadata OK: person object, fields and default view match the frontend')
PY

UNAUTH_STATUS=$(curl -s -o /tmp/squid-smoke-unauth.json -w '%{http_code}' "${BASE}/rest/people")
test "$UNAUTH_STATUS" = "403" || { echo "expected 403, got $UNAUTH_STATUS"; exit 1; }
python3 - <<'PY'
import json
body = json.load(open('/tmp/squid-smoke-unauth.json'))
assert body['statusCode'] == 403, body
assert body['error'] == 'FORBIDDEN_EXCEPTION', body
assert body['messages'] == ['Missing authentication token'], body
print('unauth OK: 403 contract matches')
PY

TOKEN="$TOKEN" BASE="$BASE" python3 - <<'PY'
import json, os, urllib.error, urllib.request, uuid

base = os.environ["BASE"]
token = os.environ["TOKEN"]

def post(path, payload, bearer):
    req = urllib.request.Request(base + path, data=json.dumps(payload).encode(), method="POST")
    req.add_header("Content-Type", "application/json")
    req.add_header("Authorization", "Bearer " + bearer)
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read())

key = post("/metadata", {
    "query": "mutation($i: CreateApiKeyInput!) { createApiKey(input: $i) { id } }",
    "variables": {"i": {"name": "smoke", "expiresAt": "2030-01-01T00:00:00Z",
                        "roleId": str(uuid.uuid4())}},
}, token)
key_id = key["data"]["createApiKey"]["id"]

generated = post("/metadata", {
    "query": "mutation($id: UUID!) { generateApiKeyToken(apiKeyId: $id, expiresAt: \"2030-01-01T00:00:00Z\") { token } }",
    "variables": {"id": key_id},
}, token)
api_token = generated["data"]["generateApiKeyToken"]["token"]

req = urllib.request.Request(base + "/rest/people", headers={"Authorization": "Bearer " + api_token})
with urllib.request.urlopen(req, timeout=15) as resp:
    assert resp.status == 200

post("/metadata", {
    "query": "mutation($i: RevokeApiKeyInput!) { revokeApiKey(input: $i) { id revokedAt } }",
    "variables": {"i": {"id": key_id}},
}, token)

try:
    urllib.request.urlopen(req, timeout=15)
    raise AssertionError("revoked API key was accepted")
except urllib.error.HTTPError as exc:
    assert exc.code == 403, exc.code

print("api key OK: create -> generate -> use -> revoke")
PY

echo "SMOKE PASSED"
