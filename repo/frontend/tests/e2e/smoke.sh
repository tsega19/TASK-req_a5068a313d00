#!/usr/bin/env sh
# Frontend end-to-end smoke test.
# Assumes the compose stack is running and the frontend is reachable at
# ${FRONTEND_URL} (default http://localhost:8081). Verifies:
#   - static HTML is served
#   - the WASM payload is served
#   - /api proxy forwards to the backend health endpoint
#   - the login → token → authorized /api/me round-trip works end-to-end

set -u

FRONTEND_URL="${FRONTEND_URL:-http://localhost:8081}"
ADMIN_USER="${ADMIN_USER:-admin}"
ADMIN_PASS="${ADMIN_PASS:-admin123}"

pass=0
fail=0
total=0

assert() {
    total=$((total + 1))
    local name="$1"; local cond="$2"
    if [ "${cond}" = "0" ]; then
        echo "  PASS ${name}"
        pass=$((pass + 1))
    else
        echo "  FAIL ${name}"
        fail=$((fail + 1))
    fi
}

echo "==== Frontend e2e smoke against ${FRONTEND_URL} ===="

# 1. Root page returns 200 with an HTML body containing the app root div.
root_body="$(curl -s -o /tmp/e2e_root "${FRONTEND_URL}/" -w '%{http_code}')"
case "$root_body" in
    200) echo "  root status 200" ;;
    *)   echo "  root status ${root_body}"; fail=$((fail + 1)); total=$((total + 1));;
esac
grep -q 'id="app-root"' /tmp/e2e_root
assert "static HTML served with #app-root" $?

# 2. nginx proxies /api/health to the backend.
proxy_body="$(curl -s -o /tmp/e2e_health "${FRONTEND_URL}/api/health" -w '%{http_code}')"
assert "api proxy returns 200 (got ${proxy_body})" "$([ "${proxy_body}" = "200" ] && echo 0 || echo 1)"

# 3. End-to-end login -> token -> /api/me round-trip.
login_code="$(curl -s -o /tmp/e2e_login \
    -X POST \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"${ADMIN_USER}\",\"password\":\"${ADMIN_PASS}\"}" \
    "${FRONTEND_URL}/api/auth/login" \
    -w '%{http_code}')"
assert "login returns 200 (got ${login_code})" "$([ "${login_code}" = "200" ] && echo 0 || echo 1)"

token="$(sed -n 's/.*"token":"\([^"]*\)".*/\1/p' /tmp/e2e_login)"
if [ -z "${token}" ]; then
    echo "  could not parse token from login response:"
    cat /tmp/e2e_login
    fail=$((fail + 1))
    total=$((total + 1))
else
    me_code="$(curl -s -o /tmp/e2e_me \
        -H "Authorization: Bearer ${token}" \
        "${FRONTEND_URL}/api/me" \
        -w '%{http_code}')"
    assert "authorized /api/me returns 200 (got ${me_code})" \
        "$([ "${me_code}" = "200" ] && echo 0 || echo 1)"
    grep -q "\"username\":\"${ADMIN_USER}\"" /tmp/e2e_me
    assert "me response contains admin username" $?
fi

echo
echo "==== e2e summary: ${pass}/${total} passed (${fail} failed) ===="
[ "${fail}" -eq 0 ]
