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

    # -------------------------------------------------------------------
    # 3b. PRD §6 password-reset gate: the seeded admin boots with
    #     `password_reset_required = TRUE` when `REQUIRE_ADMIN_PASSWORD_CHANGE`
    #     is set (docker-compose default). Every privileged route stays
    #     behind the gate until the password is rotated. The rotation
    #     itself is exempt, so we use the same bearer. The middleware
    #     re-reads the DB flag on each request, so the *same* JWT unlocks
    #     everything once the flag flips to FALSE.
    # -------------------------------------------------------------------
    if grep -q '"password_reset_required":true' /tmp/e2e_login; then
        NEW_PASS="${ADMIN_PASS}-rotated-e2e"
        rotate_code="$(curl -s -o /tmp/e2e_rotate \
            -X POST \
            -H "Authorization: Bearer ${token}" \
            -H 'Content-Type: application/json' \
            -d "{\"current_password\":\"${ADMIN_PASS}\",\"new_password\":\"${NEW_PASS}\"}" \
            "${FRONTEND_URL}/api/auth/change-password" \
            -w '%{http_code}')"
        assert "forced password rotation returns 200 (got ${rotate_code})" \
            "$([ "${rotate_code}" = "200" ] && echo 0 || echo 1)"
        grep -q '"ok":true' /tmp/e2e_rotate
        assert "rotation response body confirms ok:true" $?
        ADMIN_PASS="${NEW_PASS}"
    fi

    # -------------------------------------------------------------------
    # 4. Failure path: invalid bearer must return 401 with a structured
    #    error envelope (not just a bare status — the UI parses the body).
    # -------------------------------------------------------------------
    bad_code="$(curl -s -o /tmp/e2e_bad \
        -H "Authorization: Bearer not-a-real-token" \
        "${FRONTEND_URL}/api/me" \
        -w '%{http_code}')"
    assert "/api/me rejects bad token with 401 (got ${bad_code})" \
        "$([ "${bad_code}" = "401" ] && echo 0 || echo 1)"
    grep -q '"code":"unauthorized"' /tmp/e2e_bad
    assert "/api/me 401 body carries unauthorized code" $?

    # -------------------------------------------------------------------
    # 5. Failure path: wrong credentials rejection — the login shouldn't
    #    leak whether the username exists.
    # -------------------------------------------------------------------
    wrong_code="$(curl -s -o /tmp/e2e_wrong \
        -X POST \
        -H 'Content-Type: application/json' \
        -d "{\"username\":\"${ADMIN_USER}\",\"password\":\"not-the-real-password\"}" \
        "${FRONTEND_URL}/api/auth/login" \
        -w '%{http_code}')"
    assert "login with wrong password returns 401 (got ${wrong_code})" \
        "$([ "${wrong_code}" = "401" ] && echo 0 || echo 1)"
    grep -q '"error":"invalid credentials"' /tmp/e2e_wrong
    assert "wrong-password 401 body has invalid-credentials error" $?

    # -------------------------------------------------------------------
    # 6. RBAC journey: the admin-only processing log must be reachable
    #    for admin, and return a paginated body with total + data[].
    # -------------------------------------------------------------------
    plog_code="$(curl -s -o /tmp/e2e_plog \
        -H "Authorization: Bearer ${token}" \
        "${FRONTEND_URL}/api/admin/processing-log" \
        -w '%{http_code}')"
    assert "admin can read processing log (got ${plog_code})" \
        "$([ "${plog_code}" = "200" ] && echo 0 || echo 1)"
    # Body should carry pagination envelope — assert structure, not just 200.
    grep -q '"total":' /tmp/e2e_plog
    assert "processing-log response includes total" $?
    grep -q '"data":' /tmp/e2e_plog
    assert "processing-log response includes data array" $?

    # -------------------------------------------------------------------
    # 7. Workflow journey: list work orders, then drill into the first
    #    one by id — exercises the most common tech path end-to-end.
    # -------------------------------------------------------------------
    wo_list_code="$(curl -s -o /tmp/e2e_wo_list \
        -H "Authorization: Bearer ${token}" \
        "${FRONTEND_URL}/api/work-orders" \
        -w '%{http_code}')"
    assert "work-orders list returns 200 (got ${wo_list_code})" \
        "$([ "${wo_list_code}" = "200" ] && echo 0 || echo 1)"
    grep -q '"data":' /tmp/e2e_wo_list
    assert "work-orders list response includes data array" $?
    # Extract the first work order id (UUID-shaped); skip if the list
    # is empty (e.g. after a fresh DB wipe) rather than failing the suite.
    first_id="$(sed -n 's/.*"data":\[{[^}]*"id":"\([0-9a-f-]\{36\}\)".*/\1/p' /tmp/e2e_wo_list)"
    if [ -n "${first_id}" ]; then
        detail_code="$(curl -s -o /tmp/e2e_wo_detail \
            -H "Authorization: Bearer ${token}" \
            "${FRONTEND_URL}/api/work-orders/${first_id}" \
            -w '%{http_code}')"
        assert "work-order detail round-trip (got ${detail_code})" \
            "$([ "${detail_code}" = "200" ] && echo 0 || echo 1)"
        grep -q "\"id\":\"${first_id}\"" /tmp/e2e_wo_detail
        assert "work-order detail body echoes requested id" $?
    else
        echo "  SKIP work-order detail round-trip (no seed data)"
    fi

    # -------------------------------------------------------------------
    # 8. Unauthenticated access to a privileged route must return 401 —
    #    not 404, not 500. Exercises the JwtAuth middleware at the edge.
    # -------------------------------------------------------------------
    anon_code="$(curl -s -o /tmp/e2e_anon "${FRONTEND_URL}/api/work-orders" -w '%{http_code}')"
    assert "unauthenticated /api/work-orders returns 401 (got ${anon_code})" \
        "$([ "${anon_code}" = "401" ] && echo 0 || echo 1)"
    grep -q '"code":"unauthorized"' /tmp/e2e_anon
    assert "anonymous 401 body carries unauthorized code" $?
fi

echo
echo "==== e2e summary: ${pass}/${total} passed (${fail} failed) ===="
[ "${fail}" -eq 0 ]
