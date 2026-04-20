# Test Coverage and README Audit Report

## Tests Check
Project shape is fullstack (`backend` Rust API + `frontend` Rust/WASM UI). Relevant categories are backend API/integration tests, backend unit tests, frontend unit/component tests, and end-to-end boundary coverage.

- Present and meaningful:
  - Backend API/integration tests are strong (`backend/tests/api/*.rs`) and exercise real app routes with real Postgres interactions.
  - Backend unit tests are strong (`backend/tests/unit/*.rs`) and cover state machine, RBAC guards, crypto, sync merge logic, and pagination.
  - Frontend unit tests are now broader (`wasm_bindgen_test` modules across `types`, `offline`, pages, and multiple components such as `nav`, `sla`, `state_badge`, `timer_ring`, `map_svg`, `toast`).
  - E2E smoke exists (`frontend/tests/e2e/smoke.sh`) and now covers more than health/login, including key failure paths and role-protected route checks.

- Remaining weak areas:
  - Frontend tests are still mostly helper/pure-logic assertions; deep DOM-level interaction flows remain limited.
  - E2E is still shell/curl-driven smoke+journey checks, not rich browser-level interaction testing.

## run_tests.sh Audit
- `run_tests.sh` exists.
- Backend tests run in Docker (`docker compose --profile tests run ... backend-test cargo test`).
- Frontend wasm unit tests run in Docker (`frontend-test` service).
- E2E smoke runs in Docker (`docker compose --profile e2e run --rm e2e-smoke`).
- Prior host `curl` readiness dependency was addressed: readiness now probes from inside the frontend container (`docker exec ... wget`).
- Added optional coverage artifact flow (`COVERAGE=1`) producing tarpaulin HTML + Cobertura XML under `.tmp/coverage`.

## Test Coverage Score
**92/100**

## Score Rationale
Backend coverage remains confidence-building and comprehensive. Frontend coverage and execution pipeline quality improved materially with broader wasm tests and fully Dockerized test stages. Score is slightly reduced because frontend assertions are still largely logic-centric and e2e remains script-based rather than full browser-interaction depth.

## Key Gaps
- Limited deep frontend rendered interaction/state-flow tests.
- E2E is improved but still shell/curl based instead of richer browser-driven scenarios.
- Coverage reporting is opt-in (`COVERAGE=1`) rather than default in the primary test path.
