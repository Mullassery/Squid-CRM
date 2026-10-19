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

echo "SMOKE PASSED"
