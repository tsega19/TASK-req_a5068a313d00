#!/usr/bin/env bash
# FieldOps — global test runner. Executes every test suite inside Docker
# and prints a Total / Passed / Failed summary with per-stage outcomes.

set -u
cd "$(dirname "$0")"

PASS=0
FAIL=0
TOTAL=0
LOGS=()

run_stage() {
    local name="$1"; shift
    TOTAL=$((TOTAL + 1))
    echo
    echo "================================================================"
    echo "[run_tests] Stage: ${name}"
    echo "================================================================"
    if "$@"; then
        PASS=$((PASS + 1))
        LOGS+=("PASS ${name}")
    else
        FAIL=$((FAIL + 1))
        LOGS+=("FAIL ${name}")
    fi
}

# ----------------------------------------------------------------------------
# 1. Start Postgres (tests need a live DB for integration + unit suites).
# ----------------------------------------------------------------------------
echo "[run_tests] Bringing up postgres..."
docker compose up -d postgres

echo "[run_tests] Waiting for postgres to report healthy..."
for i in $(seq 1 30); do
    status="$(docker inspect -f '{{.State.Health.Status}}' fieldops_postgres 2>/dev/null || echo starting)"
    if [ "${status}" = "healthy" ]; then
        echo "[run_tests] postgres is healthy."
        break
    fi
    sleep 2
done

# ----------------------------------------------------------------------------
# 2. Backend unit + API tests (single invocation — cargo discovers both
#    `tests/unit.rs` and `tests/api.rs` binaries plus inline src/ tests).
# ----------------------------------------------------------------------------
run_stage "backend unit + api tests" \
    docker compose --profile tests run --rm --build \
        backend-test cargo test --release -- --test-threads=1

# ----------------------------------------------------------------------------
# 3. Frontend end-to-end smoke — opt-in because it requires the full stack.
#    Enabled by default when BACKEND + FRONTEND images can be built, skipped
#    otherwise with a clear notice.
# ----------------------------------------------------------------------------
if [ "${SKIP_E2E:-0}" = "1" ]; then
    echo
    echo "[run_tests] SKIP_E2E=1 — skipping frontend e2e smoke stage"
    LOGS+=("SKIP frontend e2e (SKIP_E2E=1)")
else
    echo "[run_tests] Bringing up full stack for e2e..."
    docker compose up -d --build backend frontend

    # Wait for the frontend to serve HTML.
    ready=0
    for i in $(seq 1 30); do
        if curl -fsS -o /dev/null http://localhost:8081/; then
            ready=1
            break
        fi
        sleep 2
    done

    if [ "${ready}" -ne 1 ]; then
        echo "[run_tests] Frontend did not come up in time — dumping container state."
        docker compose ps
        echo "---- fieldops_backend logs (tail 60) ----"
        docker logs --tail 60 fieldops_backend 2>&1 || true
        echo "---- fieldops_frontend logs (tail 60) ----"
        docker logs --tail 60 fieldops_frontend 2>&1 || true
        TOTAL=$((TOTAL + 1))
        FAIL=$((FAIL + 1))
        LOGS+=("FAIL frontend e2e (timeout)")
    else
        run_stage "frontend e2e smoke" \
            sh frontend/tests/e2e/smoke.sh
    fi
fi

# ----------------------------------------------------------------------------
# Summary
# ----------------------------------------------------------------------------
echo
echo "================================================================"
echo "Summary"
echo "================================================================"
for line in "${LOGS[@]}"; do
    printf "  %s\n" "${line}"
done
echo
echo "Total:  ${TOTAL}"
echo "Passed: ${PASS}"
echo "Failed: ${FAIL}"

if [ "${FAIL}" -gt 0 ]; then
    exit 1
fi
exit 0
