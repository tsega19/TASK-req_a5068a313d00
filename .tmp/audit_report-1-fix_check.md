# Retest Result - delivery_acceptance_architecture_audit_fix.md

Date: 2026-04-21
Mode: Static-only retest (no runtime execution, no Docker, no tests run)

## Verdict
Partial Pass

## Summary
The previously reported material issues are largely fixed in code and documentation. One material residual gap remains in **test coverage depth** for SUPER cross-branch conflict enforcement (implementation present; regression tests appear incomplete for explicit negative scope checks on conflict list/resolve).

## Re-verified Fixes (Evidence)

1. Blocker: Admin UI branch requirement for TECH/SUPER - Fixed
- Validation requires branch for TECH/SUPER: `repo/frontend/src/pages/admin.rs:21-34`
- Branch selector wired in form state/UI: `repo/frontend/src/pages/admin.rs:82`, `repo/frontend/src/pages/admin.rs:216`
- Request includes `branch_id` only when parseable UUID: `repo/frontend/src/pages/admin.rs:154-157`
- Validation tests for TECH/SUPER branch-required behavior: `repo/frontend/src/pages/admin.rs:568-584`

2. High: Automatic on-call routing - Fixed
- Migration adds durable `on_call` + index: `repo/backend/migrations/0006_on_call_routing.sql:8-12`
- Routing rule helper: `repo/backend/src/work_orders/routes.rs:67`
- Create persists `on_call` and logs route action: `repo/backend/src/work_orders/routes.rs:304-332`, `repo/backend/src/work_orders/routes.rs:363-376`
- Transition recalculates/persists `on_call`: `repo/backend/src/work_orders/routes.rs:523-542`
- Queue reads persisted `on_call = TRUE`: `repo/backend/src/work_orders/routes.rs:193`

3. High: If-Match precondition enforcement - Fixed
- 412 error type introduced: `repo/backend/src/errors.rs:21-25`, `repo/backend/src/errors.rs:74`
- Shared precondition helper: `repo/backend/src/work_orders/routes.rs:35-56`
- Transition endpoint enforces If-Match: `repo/backend/src/work_orders/routes.rs:415`
- Progress endpoint enforces on updates: `repo/backend/src/work_orders/progress.rs:98-103`
- API regression tests for missing/stale If-Match: `repo/backend/tests/api/work_orders.rs:335`, `repo/backend/tests/api/work_orders.rs:350`
- Frontend header propagation: `repo/frontend/src/api.rs:83-114`, `repo/frontend/src/offline.rs:374`, `repo/frontend/src/pages/work_order_detail.rs:398`

4. High: API spec alignment - Fixed
- on_call documented: `docs/api_aspec.md:42`
- endpoint corrected to `PUT /{id}/state`: `docs/api_aspec.md:93`
- If-Match contract documented: `docs/api_aspec.md:105`

5. Medium: SUPER conflict scoping - Implementation fixed, tests partially evidenced
- SUPER-scoped conflict list logic: `repo/backend/src/sync/routes.rs:118-142`
- SUPER-scoped resolve guard (404 on scope miss): `repo/backend/src/sync/routes.rs:174-193`
- Existing sync tests include role/flow checks, but explicit negative SUPER cross-branch conflict list/resolve assertions are not clearly present: `repo/backend/tests/api/sync.rs:348-393`, `repo/backend/tests/api/sync.rs:447-475`

6. Medium: Synthetic location fallback removal - Fixed
- Capture fails closed on geolocation failure; no synthetic point posting: `repo/frontend/src/pages/map_view.rs:126-164`

## Remaining Gap
- Severity: Medium
- Title: Missing explicit regression tests for SUPER cross-branch conflict denial
- Impact: Scope regressions in conflict list/resolve could reappear without being caught by tests.
- Minimum fix: Add API tests asserting SUPER cannot list or resolve conflicts tied to other branches, expecting filtered list and 404 on resolve.

## Manual Verification Required
- UI smoke for branch selector behavior and error messaging.
- End-to-end migration/application behavior for new `on_call` column.
- Runtime UX for stale ETag -> user-visible 412 handling.
