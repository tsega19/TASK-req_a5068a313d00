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
# 3. Frontend wasm unit tests — Dockerized via the `tester` stage in
#    frontend/Dockerfile. Runs every #[wasm_bindgen_test] through
#    wasm-bindgen-test-runner + Node inside the container. No host toolchain
#    dependency, no wasm-pack install on the developer's machine.
# ----------------------------------------------------------------------------
run_stage "frontend wasm unit tests" \
    docker compose --profile tests run --rm --build frontend-test

# ----------------------------------------------------------------------------
# 3b. Backend coverage artifact (opt-in via COVERAGE=1). Produces an HTML
#     report + a Cobertura XML under .tmp/coverage/ so CI or a human can
#     cite a real line/branch % instead of eyeballing test counts. Skipped
#     by default because tarpaulin's instrumented rebuild adds minutes.
# ----------------------------------------------------------------------------
if [ "${COVERAGE:-0}" = "1" ]; then
    mkdir -p .tmp/coverage
    run_stage "backend coverage (tarpaulin)" \
        docker compose --profile coverage run --rm backend-coverage
    # Surface the headline number so CI logs carry a self-contained summary.
    if [ -f .tmp/coverage/cobertura.xml ]; then
        pct="$(sed -n 's/.*line-rate="\([0-9.]*\)".*/\1/p' .tmp/coverage/cobertura.xml | head -n1)"
        echo "[run_tests] backend line coverage: ${pct:-?} (see .tmp/coverage/tarpaulin-report.html)"
    fi
fi

# ----------------------------------------------------------------------------
# 4. Frontend end-to-end smoke — opt-in because it requires the full stack.
#    Enabled by default when BACKEND + FRONTEND images can be built, skipped
#    otherwise with a clear notice.
# ----------------------------------------------------------------------------
if [ "${SKIP_E2E:-0}" = "1" ]; then
    echo
    echo "[run_tests] SKIP_E2E=1 — skipping frontend e2e smoke stage"
    LOGS+=("SKIP frontend e2e (SKIP_E2E=1)")
else
    # The backend integration suite truncates+reseeds users with a test
    # fixture admin (password hash of "pw"). That row persists in postgres
    # after the tests exit and shadows the docker-compose DEFAULT_ADMIN_PASSWORD
    # because `seed_default_admin` skips when a row with the same username
    # already exists. Wipe it here so the backend reseeds cleanly and the
    # e2e smoke can log in as admin/admin123.
    echo "[run_tests] Wiping test-leftover admin row before e2e boot..."
    docker exec fieldops_postgres psql -U fieldops -d fieldops \
        -v ON_ERROR_STOP=0 \
        -c "DELETE FROM users WHERE username = 'admin';" >/dev/null 2>&1 || true

    echo "[run_tests] Bringing up full stack for e2e..."
    docker compose up -d --build --force-recreate backend frontend

    # Wait for the frontend to serve HTML. Previously this loop used the
    # host's `curl`, which was the last host-dependency in run_tests.sh.
    # The frontend container (nginx:alpine) ships busybox `wget`, so we
    # poll from *inside* the container and drop the host requirement.
    ready=0
    for i in $(seq 1 30); do
        if docker exec fieldops_frontend wget -q --spider http://127.0.0.1/ >/dev/null 2>&1; then
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
        # Fully Dockerized e2e: runs inside the compose network so the smoke
        # reaches the frontend via its service name (http://frontend) and
        # never depends on the host's curl or port forwarding.
        run_stage "frontend e2e smoke" \
            docker compose --profile e2e run --rm e2e-smoke
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
