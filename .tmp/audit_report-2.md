# Static Delivery Acceptance & Architecture Audit — FieldOps Kitchen & Training Console

Date: 2026-04-18  
Audit type: static-only (no execution, no Docker, no tests run)

## 1. Verdict

- **Overall conclusion: Partial Pass**
  - The repository provides a coherent full-stack scaffold (Actix-web backend + Yew frontend + Postgres schema + integration/unit tests) and implements several core flows (auth, RBAC, work order state machine, step progress + timer snapshot persistence, tip cards, map/trail privacy).
  - However, multiple **Prompt-critical requirements are missing or only stubbed**, especially the learning/quiz pipeline (knowledge points + learning records capture), true offline-first replication semantics, notification retry/receipts behavior, and real location trajectory capture.

## 2. Scope and Static Verification Boundary

### What was reviewed
- Backend wiring + routes + core modules: `repo/backend/src/main.rs:35`, `repo/backend/src/lib.rs:37`
- Configuration + secrets/dev-mode gates: `repo/backend/config/mod.rs:153`, `repo/docker-compose.yml:58`
- Database schema + immutability trigger: `repo/backend/migrations/0001_init.sql:136`, `repo/backend/migrations/0001_init.sql:176`
- Frontend main pages for workflow + timers + map view: `repo/frontend/src/pages/work_order_detail.rs:20`, `repo/frontend/src/pages/recipe_step.rs:28`, `repo/frontend/src/pages/map_view.rs:22`
- Tests + harness + scripts (static review only): `repo/backend/tests/api/common.rs:38`, `repo/run_tests.sh:49`

### What was not reviewed
- Runtime behavior, networking, browser/device integration, performance, and real deployment hardening.
- Docker build/run behavior (explicitly not executed per audit boundary).

### What was intentionally not executed
- No `docker compose`, no app startup, no migrations run, no tests run.

### Claims requiring manual verification
- Any “offline-first” behavior across machines/replicas (real sync topology, data loss, conflict UX): `repo/backend/src/sync/routes.rs:38`
- Audible reminders in real browsers/tablets (autoplay restrictions, device audio policy): `repo/frontend/src/components/timer_ring.rs:194`
- Frontend tablet UX and actual map rendering fidelity (SVG is static; no real mapping provider): `repo/frontend/src/components/map_svg.rs:1` (manual)

## 3. Repository / Requirement Mapping Summary

### Prompt core goal & flows (condensed)
- Tablet-optimized technician console to execute standardized work orders via step-by-step “recipe” workflows, with **multiple concurrent timers** and reminders; steps can be paused/resumed without losing timers/notes.
- Work orders as a **state machine** (Scheduled → … → Completed/Canceled) with required fields per transition, routing rules, SLA timers/alerts, and immutable processing logs.
- Map-style job view with technician trajectory, arrival/departure check-ins, and privacy mode to reduce precision/hide trail from non-admins.
- Supervisor/admin learning analytics with filters (MM/DD/YYYY), CSV export + watermark, and role-based visibility.
- Offline-first APIs with deterministic sync, ETag-style change detection, soft deletes (90-day retention), and version history retention (30 per record); deterministic conflict resolution with supervisor review.
- In-app-only notification center in offline mode with templates, delivery/read receipts, retry/backoff, unsubscribes, and rate limiting.

### Main implementation areas mapped
- Backend: auth + JWT middleware, RBAC checks, work order CRUD + state transition log, progress snapshots + versioning, sync trigger + merge policy, location trail + privacy, analytics CSV export.
- Frontend: Yew pages for login/dashboard/work orders/step timers/tip cards/map trail/analytics.

## 4. Section-by-section Review

### 1.1 Documentation and static verifiability
- **Conclusion: Partial Pass**
- **Rationale:** Clear Docker-based startup and ports are documented, plus test entrypoint exists, but docs are heavily centered on Docker (and this audit did not execute). Static consistency between docs and compose/config appears reasonable.
- **Evidence:** `repo/README.md:5`, `repo/README.md:13`, `repo/docker-compose.yml:18`, `repo/run_tests.sh:49`
- **Manual verification:** Running stack and tests requires Docker; not executed in this audit.

### 1.2 Material deviation from the Prompt
- **Conclusion: Partial Pass**
- **Rationale:** Core FieldOps work order workflow exists, but major Prompt pillars are missing/partial (learning pipeline, true replica sync semantics, notification retry/receipts, real trajectory capture). These are not minor; they materially affect the stated business goal.
- **Evidence:** Missing learning record write paths: `repo/backend/migrations/0001_init.sql:268` vs no routes beyond analytics: `repo/backend/src/analytics/routes.rs:52` and no other references: `repo/backend/src/analytics/routes.rs:88`
- **Manual verification:** N/A (missing is statically observable).

---

### 2.1 Core requirement coverage (Prompt)
- **Conclusion: Partial Pass**
- **Rationale (implemented):**
  - Work order states + transition validation + required fields (GPS, notes, check-ins, step completion gate): `repo/backend/src/state_machine.rs:23`, `repo/backend/src/work_orders/routes.rs:273`
  - Immutable transition log (“processing log”): `repo/backend/migrations/0001_init.sql:176`, timeline endpoint: `repo/backend/src/work_orders/routes.rs:415`
  - Step-by-step recipe steps + pause/resume + persistent notes and timer snapshot field: `repo/backend/migrations/0001_init.sql:191`, `repo/backend/src/work_orders/progress.rs:65`
  - Multiple concurrent timers per step (data model + API + UI): `repo/backend/migrations/0001_init.sql:109`, `repo/backend/src/recipes/routes.rs:123`, `repo/frontend/src/pages/recipe_step.rs:215`
  - Tip cards pinned to steps, admin-authored: `repo/backend/migrations/0001_init.sql:121`, `repo/backend/src/recipes/routes.rs:162`
  - Map/trail + privacy mode with precision reduction and role-based hiding: `repo/backend/src/location/routes.rs:110`, `repo/backend/src/geo.rs:15`, `repo/frontend/src/pages/map_view.rs:70`
  - Learning analytics filters (MM/DD/YYYY) + CSV export + watermark: `repo/backend/src/analytics/routes.rs:42`, `repo/backend/src/analytics/routes.rs:123`
  - In-app notifications list + read + unsubscribe + rate limiting (stubbed delivery): `repo/backend/src/notifications/routes.rs:36`, `repo/backend/src/notifications/stub.rs:68`
- **Rationale (missing / partial):**
  - No implemented “knowledge points” authoring, quiz delivery, or learning record capture endpoints (schema exists, no write-paths): `repo/backend/migrations/0001_init.sql:251`, `repo/backend/src/analytics/routes.rs:79`
  - Notification retry/backoff and delivery mechanics are not implemented beyond helper + “always succeeds” stub: `repo/backend/src/notifications/stub.rs:103`, `repo/backend/src/notifications/stub.rs:126`
  - Soft-delete retention (90 days) is not enforced by any cleanup job; retention is config-only: `repo/docker-compose.yml:77`, `repo/backend/config/mod.rs:220`
  - Version retention “30 versions per record” appears implemented for **step progress only**, not generally for all record types: `repo/backend/src/work_orders/progress.rs:161`, `repo/backend/migrations/0001_init.sql:209`
  - “Technician’s recorded trajectory” is stubbed on frontend (synthetic jitter, no geolocation integration): `repo/frontend/src/pages/map_view.rs:126`
- **Manual verification:** Where behavior depends on runtime background jobs/timers (sync ticker, notification retries), manual verification required.

### 2.2 End-to-end deliverable (0→1)
- **Conclusion: Pass**
- **Rationale:** Repo contains backend, frontend, schema migrations, compose, and tests; structure resembles a minimal product deliverable rather than a single-file snippet.
- **Evidence:** `repo/backend/src/main.rs:9`, `repo/frontend/src/main.rs:1`, `repo/backend/migrations/0001_init.sql:1`, `repo/docker-compose.yml:18`

---

### 3.1 Engineering structure and decomposition
- **Conclusion: Pass**
- **Rationale:** Clear module split (auth, middleware, work_orders, recipes, sync, location, analytics) with shared route registration via `configure`.
- **Evidence:** `repo/backend/src/lib.rs:12`, `repo/backend/src/lib.rs:37`, `repo/frontend/src/pages/mod.rs:1`

### 3.2 Maintainability and extensibility
- **Conclusion: Partial Pass**
- **Rationale:** Many core concepts are separated cleanly (state machine, sync merge policy, redacting logger). However, some prompt-critical systems are incomplete (learning, notification retries, retention cleanup), which limits extensibility for the stated business objective.
- **Evidence:** Sync merge invariants documented and enforced: `repo/backend/src/sync/merge.rs:3`; missing learning write paths: `repo/backend/migrations/0001_init.sql:268` with no routes besides read-only rollup: `repo/backend/src/analytics/routes.rs:110`

---

### 4.1 Engineering details (error handling, logging, validation, API design)
- **Conclusion: Partial Pass**
- **Rationale (strengths):**
  - Uniform JSON error type with standard status mapping: `repo/backend/src/errors.rs:54`
  - Request logging middleware avoids logging bodies; structured logging with redaction: `repo/backend/src/middleware/request_log.rs:1`, `repo/backend/logging/mod.rs:37`
  - Validation exists for high-risk operations (RBAC, state transitions, radius checks, password change length): `repo/backend/src/work_orders/routes.rs:166`, `repo/backend/src/auth/routes.rs:111`
- **Rationale (gaps/risks):**
  - `ApiError::Internal` may include raw database error strings (potential info leak): `repo/backend/src/errors.rs:74`
  - Work-order transition required-fields logged as booleans only (lat/lng presence) rather than a richer audit record; prompt asks “every transition and user action writes an immutable processing log” (only state transitions are immutable; other actions are not clearly logged immutably): `repo/backend/migrations/0001_init.sql:164`, `repo/backend/src/work_orders/progress.rs:203`
- **Manual verification:** Whether operational logs are sufficient for real troubleshooting depends on runtime log configuration; not executed.

### 4.2 “Real product” organization (vs demo)
- **Conclusion: Partial Pass**
- **Rationale:** The project has a product-like shape (RBAC, migrations, tests, UI pages), but missing learning capture and true offline replication mechanics indicates partial implementation relative to prompt scope.
- **Evidence:** Present: `repo/backend/src/work_orders/routes.rs:32`, missing: no learning record endpoints: `repo/backend/src/analytics/routes.rs:52`

---

### 5.1 Prompt understanding and semantic fit
- **Conclusion: Partial Pass**
- **Rationale:** Many semantics match the prompt closely (state machine gating with check-ins, privacy mode, multi-timers). But some key semantics are replaced by stubs (trajectory capture) or absent (learning unit/knowledge point lifecycle).
- **Evidence:** Privacy hide-from-supervisors behavior: `repo/backend/src/location/routes.rs:133`; stubbed capture point: `repo/frontend/src/pages/map_view.rs:126`; no knowledge point routes: `repo/backend/migrations/0001_init.sql:251`

---

### 6.1 Aesthetics (frontend-only)
- **Conclusion: Cannot Confirm Statistically**
- **Rationale:** CSS exists and pages are structured, but visual quality and tablet optimization require runtime rendering review.
- **Evidence:** Styles present: `repo/frontend/styles/main.css:1`; UI composition: `repo/frontend/src/pages/work_order_detail.rs:81`
- **Manual verification:** Open frontend on tablet-size viewport, validate spacing, hierarchy, touch target sizing, and timer/map interactions.

## 5. Issues / Suggestions (Severity-Rated)

### Blocker

1) **Learning pipeline missing (knowledge points, quizzes, learning record capture)**
- **Conclusion:** Missing core functionality vs Prompt
- **Evidence:** Schema exists: `repo/backend/migrations/0001_init.sql:251` and `repo/backend/migrations/0001_init.sql:268`; only rollup read endpoints exist: `repo/backend/src/analytics/routes.rs:110`
- **Impact:** Supervisors cannot get real completion/quiz/time/review metrics tied to workflows because there is no implemented mechanism to author knowledge points/quizzes or record technician interactions.
- **Minimum actionable fix:** Add backend endpoints and frontend UI flows to (a) create/manage `knowledge_points`, (b) deliver quiz prompts at relevant steps, and (c) write `learning_records` with quiz score, time spent, review count, completion timestamps; add RBAC + tests.

2) **Offline-first “replica sync between local replicas” is incomplete**
- **Conclusion:** Partial implementation; core prompt claim not met end-to-end
- **Evidence:** Sync exists for step progress push + merge: `repo/backend/src/sync/routes.rs:38`, `repo/backend/src/sync/merge.rs:62`; scheduled ticker only recomputes ETags locally: `repo/backend/src/lib.rs:63`
- **Impact:** Prompt describes scheduled sync between replicas with soft deletes + record version retention and deterministic merge policy. Current code shows a merge policy for a single entity type (`job_step_progress`) and local etag scanning; it does not demonstrate multi-replica pull/push exchange, broader entity coverage, or delete/version retention enforcement.
- **Minimum actionable fix:** Define the replica protocol (pull changes since cursor, push local changes, conflict listing/resolution) and extend merge/ETag coverage across required entities (work orders, recipes/tip cards as needed); implement soft delete propagation and retention pruning.

### High

3) **Notification retry/backoff and receipts are not implemented (delivery is “always succeeds” stub)**
- **Conclusion:** Partially implemented vs Prompt
- **Evidence:** Stub explicitly “always returns success”: `repo/backend/src/notifications/stub.rs:1`; `backoff_seconds` helper exists but unused for retries: `repo/backend/src/notifications/stub.rs:126`; API only supports list/read/unsubscribe: `repo/backend/src/notifications/routes.rs:36`
- **Impact:** Prompt requires delivery/read receipts, retry with exponential backoff up to 5 attempts, and offline in-app notification semantics. Current implementation stores rows and allows read tracking, but does not implement retry attempts/backoff loops or delivery failure handling.
- **Minimum actionable fix:** Add a background job/worker to process pending notifications, increment `retry_count`, apply `backoff_seconds`, record `delivered_at` only on actual delivery, and add tests around rate limits/unsubscribes/retry cap.

4) **Location trajectory capture is stubbed in the frontend**
- **Conclusion:** Implemented as synthetic jitter rather than true device trajectory
- **Evidence:** “Synthesize a point near the job location… Real browsers can integrate navigator.geolocation later”: `repo/frontend/src/pages/map_view.rs:126`
- **Impact:** The central “recorded trajectory for the visit” requirement is not met as described; analytics/audit quality and privacy mode semantics are weakened if data is synthetic.
- **Minimum actionable fix:** Integrate browser geolocation (with explicit permission UX), persist periodic sampling cadence, and add a clear offline behavior when geolocation is unavailable; ensure privacy mode affects stored precision as implemented server-side.

5) **Soft-delete retention window is not enforced**
- **Conclusion:** Config-only; no pruning logic found
- **Evidence:** Retention configured: `repo/docker-compose.yml:77`, `repo/backend/config/mod.rs:220`; soft delete fields exist: `repo/backend/migrations/0001_init.sql:153`; no cleanup job located in backend modules list: `repo/backend/src/lib.rs:12`
- **Impact:** Prompt requires a 90-day retention window; without pruning, storage can grow unbounded and “retention” is only a documented intent.
- **Minimum actionable fix:** Add a scheduled pruning job that hard-deletes rows beyond retention where appropriate (or archives), with careful handling for immutable logs; add tests for retention cutoff.

### Medium

6) **Analytics is read-only and not demonstrably tied to workflow knowledge units**
- **Conclusion:** Partial alignment
- **Evidence:** Analytics query aggregates `learning_records` but no write-path exists: `repo/backend/src/analytics/routes.rs:79`
- **Impact:** Even if analytics endpoints exist, they cannot reflect real training/quiz data without record capture.
- **Minimum actionable fix:** Implement learning record capture (see Blocker #1) and add fixtures/tests validating filters (from/to/branch/role) against seeded records.

7) **Potential sensitive information exposure via internal error strings**
- **Conclusion:** Suspected risk (static)
- **Evidence:** `ApiError::from(sqlx::Error)` returns `Internal(format!("database error: {}", other))`: `repo/backend/src/errors.rs:74`
- **Impact:** Depending on SQLx error formatting, responses may expose schema/table details or query fragments to clients.
- **Minimum actionable fix:** Map database errors to generic messages in production mode; log full details server-side only (with redaction).

### Low

8) **JWT logout is public and stateless**
- **Conclusion:** Likely acceptable, but may mislead API consumers
- **Evidence:** Middleware treats `/api/auth/logout` as public: `repo/backend/src/middleware/rbac.rs:79`; logout does not revoke tokens: `repo/backend/src/auth/routes.rs:91`
- **Impact:** Consumers might assume logout invalidates tokens. With stateless JWT, tokens remain valid until expiry.
- **Minimum actionable fix:** Document token revocation semantics clearly; optionally require auth for logout and implement revocation list if needed.

## 6. Security Review Summary

### Authentication entry points
- **Conclusion: Pass**
- **Evidence:** Login endpoint and password verification: `repo/backend/src/auth/routes.rs:42`, `repo/backend/src/auth/hashing.rs:21`

### Route-level authorization
- **Conclusion: Partial Pass**
- **Evidence:** Global JWT middleware blocks non-public routes: `repo/backend/src/middleware/rbac.rs:76`; explicit role checks used on admin/sensitive endpoints: `repo/backend/src/admin/routes.rs:49`, `repo/backend/src/work_orders/routes.rs:117`
- **Rationale:** Many endpoints have explicit checks, but enforcement relies on consistent handler usage; static audit found multiple correct examples.

### Object-level authorization (BOLA/IDOR)
- **Conclusion: Pass**
- **Evidence:** Work order visibility returns 404 when not visible: `repo/backend/src/work_orders/routes.rs:465`; notifications restricted by `user_id`: `repo/backend/src/notifications/routes.rs:44`; location trail requires `load_visible` and tech ownership check for writes: `repo/backend/src/location/routes.rs:72`

### Function-level authorization
- **Conclusion: Pass**
- **Evidence:** `require_role` / `require_any_role` helpers and usage: `repo/backend/src/middleware/rbac.rs:163`, `repo/backend/src/recipes/routes.rs:168`

### Tenant / user data isolation
- **Conclusion: Partial Pass**
- **Evidence:** Roles include `branch_id` and some scoping is enforced (work order list for SUPER): `repo/backend/src/work_orders/routes.rs:63`, analytics branch scope narrowing: `repo/backend/src/analytics/routes.rs:65`
- **Rationale:** There is no explicit multi-tenant model beyond branch scoping; some “unscoped supers see all branch-less work orders” behavior exists and should be confirmed against business rules: `repo/backend/src/work_orders/routes.rs:481`

### Admin / internal / debug endpoint protection
- **Conclusion: Pass**
- **Evidence:** Admin scope requires `Role::Admin`: `repo/backend/src/admin/routes.rs:310`; sync conflicts resolve requires SUPER/ADMIN: `repo/backend/src/sync/routes.rs:97`

## 7. Tests and Logging Review

### Unit tests
- **Conclusion: Pass (present)**
- **Evidence:** Inline unit tests exist (state machine, crypto, etag, logging redaction): `repo/backend/src/state_machine.rs:117`, `repo/backend/src/crypto.rs:58`, `repo/backend/src/etag.rs:20`, `repo/backend/logging/mod.rs:93`

### API / integration tests
- **Conclusion: Pass (present), Partial Pass (coverage)**
- **Evidence:** Actix integration tests with seeded DB harness: `repo/backend/tests/api/common.rs:38`, auth tests: `repo/backend/tests/api/auth.rs:14`, RBAC matrix: `repo/backend/tests/api/rbac.rs:15`, work order tests: `repo/backend/tests/api/work_orders.rs:7`

### Logging categories / observability
- **Conclusion: Pass**
- **Evidence:** Request/response logging middleware: `repo/backend/src/middleware/request_log.rs:60`; structured logger with redaction rules: `repo/backend/logging/mod.rs:13`

### Sensitive-data leakage risk in logs / responses
- **Conclusion: Partial Pass**
- **Evidence:** Redaction exists for common keys/tokens: `repo/backend/logging/mod.rs:14`; but internal error strings can include database error text: `repo/backend/src/errors.rs:74`

## 8. Test Coverage Assessment (Static Audit)

### 8.1 Test Overview
- **Unit tests exist:** Yes (Rust `#[test]` + `#[actix_web::test]` in modules). Evidence: `repo/backend/src/geo.rs:22`
- **API/integration tests exist:** Yes (Actix test harness + DB). Evidence: `repo/backend/tests/api/common.rs:38`
- **Test framework(s):** Rust built-in test harness + Actix test utilities. Evidence: `repo/backend/tests/api/auth.rs:6`
- **Test entry points:** Docker-based runner and direct `cargo test` via docker compose. Evidence: `repo/run_tests.sh:49`
- **Documentation provides test commands:** Yes (calls `./run_tests.sh`). Evidence: `repo/README.md:21`

### 8.2 Coverage Mapping Table

| Requirement / Risk Point | Mapped Test Case(s) | Key Assertion / Fixture / Mock | Coverage | Gap | Minimum Test Addition |
|---|---|---|---|---|---|
| Unauthed requests rejected (401) | `repo/backend/tests/api/auth.rs:73` | `GET /api/work-orders` returns 401 | Basically covered | Narrow set of endpoints | Add a small table-driven suite across admin/sync/notifications |
| Login success/failure | `repo/backend/tests/api/auth.rs:15` | token present + role | Sufficient | None | Add lockout/throttle tests if required |
| Change password requires bearer + min length | `repo/backend/tests/api/auth.rs:141` | 400 weak pw; 401 no bearer | Basically covered | No “wrong current password” test | Add negative-path test for wrong current password |
| RBAC matrix on sensitive routes | `repo/backend/tests/api/rbac.rs:16` | expected status per role | Basically covered | Matrix is incomplete (no sync/notifications) | Extend matrix to sync conflicts + notifications read |
| Object-level auth (tech cannot view other tech WO) | `repo/backend/tests/api/work_orders.rs:48` | 404 for non-owner | Sufficient | None | Add object-level tests for location trail + progress mutation |
| State transition required fields (GPS, notes) | `repo/backend/tests/api/work_orders.rs:107` | 400 without GPS | Basically covered | Missing arrival/departure check-in gating tests | Add tests for OnSite requires ARRIVAL and Completed requires DEPARTURE |
| Radius validation | `repo/backend/tests/api/work_orders.rs:90` | 400 out of radius | Basically covered | No check-in radius test | Add `POST /check-in` ARRIVAL out-of-radius test |
| Progress upsert creates/updates + version increments | `repo/backend/tests/api/work_orders.rs:246` | version 1 then 2 | Basically covered | No timer snapshot persistence assertions | Add assertions that `timer_state_snapshot` persists through GET `/progress` |
| Sync merge conflict invariants | `repo/backend/tests/unit/sync_conflicts.rs:64` | conflict logged, older rejected | Basically covered | No API-level conflict resolve test | Add integration test for `POST /api/sync/conflicts/{id}/resolve` |
| Logging redaction | `repo/backend/logging/mod.rs:97` | password/token removed | Sufficient | None | N/A |
| Learning analytics filters + CSV watermark | *(no tests found)* | N/A | Missing | No analytics tests | Add tests for date parsing, scoping, and CSV footer watermark |
| Notification rate limit/unsubscribe/retry | *(no tests found)* | N/A | Missing | Core prompt behavior absent/untested | Add tests after implementing retry worker; currently only stub send exists |

### 8.3 Security Coverage Audit
- **Authentication:** Basically covered (login success/fail, bearer required). Evidence: `repo/backend/tests/api/auth.rs:15`
- **Route authorization:** Basically covered (RBAC matrix subset). Evidence: `repo/backend/tests/api/rbac.rs:16`
- **Object-level authorization:** Basically covered (work order 404). Evidence: `repo/backend/tests/api/work_orders.rs:48`
- **Tenant/data isolation (branch scoping):** Basically covered (super sees branch jobs). Evidence: `repo/backend/tests/api/work_orders.rs:34`
- **Admin/internal protection:** Basically covered (RBAC matrix includes admin routes). Evidence: `repo/backend/tests/api/rbac.rs:30`

### 8.4 Final Coverage Judgment
- **Partial Pass**
  - Core auth/RBAC/work-order flows have meaningful test coverage.
  - Major Prompt-critical systems (learning pipeline, analytics correctness, notifications retry/receipts, retention cleanup) are missing or untested; tests could pass while severe business-critical gaps remain.

## 9. Final Notes

- The backend demonstrates strong patterns for RBAC + object scoping (404 on scope miss) and an explicit deterministic sync merge policy for step progress.
- The largest delivery risk is **functional incompleteness** relative to the business goal (training/learning), not code organization.
- Any claims about offline behavior, reminders, and UI usability must be validated manually at runtime; this report does not infer runtime success.

