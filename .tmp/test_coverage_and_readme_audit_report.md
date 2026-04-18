# Combined Audit Report: Test Coverage + README

Audit mode: **static inspection only** (no execution).
Scope (minimal): `repo/README.md`, `repo/run_tests.sh`, `repo/backend/src/**/routes.rs`, `repo/backend/src/lib.rs`, `repo/backend/src/work_orders/progress.rs`, `repo/backend/tests/**`, `repo/docker-compose.yml`.

---

## Part 1 — Test Coverage & Sufficiency Audit

### 1) Backend Endpoint Inventory (METHOD + fully resolved PATH)

Route registration: `repo/backend/src/lib.rs::configure` → service scopes from each module's `scope()` / sub-scope helpers. Individual handlers declare paths via `#[get/post/put/delete("…")]`.

**Health** (registered directly in `lib.rs::configure`)
- `GET /health`  (`repo/backend/src/lib.rs::health`)
- `GET /api/health` (`repo/backend/src/lib.rs::health`)

**Auth** (`web::scope("/api/auth")` — `repo/backend/src/auth/routes.rs::scope`)
- `POST /api/auth/login` (`auth/routes.rs:47`)
- `POST /api/auth/logout` (`auth/routes.rs:96`)
- `POST /api/auth/change-password` (`auth/routes.rs:112`)

**Me** (`web::scope("/api/me")` — `repo/backend/src/me/routes.rs::scope`)
- `GET /api/me` (`me/routes.rs:45`)
- `PUT /api/me/privacy` (`me/routes.rs:68`)
- `PUT /api/me/home-address` (`me/routes.rs:87`)
- `GET /api/me/home-address` (`me/routes.rs:114`)

**Work Orders** (`web::scope("/api/work-orders")` — `repo/backend/src/work_orders/routes.rs::scope`; nested handlers from `progress.rs` and `location/routes.rs` mounted on the same scope)
- `GET /api/work-orders` (`work_orders/routes.rs:32`)
- `GET /api/work-orders/on-call-queue` (`work_orders/routes.rs:111`)
- `GET /api/work-orders/{id}` (`work_orders/routes.rs:138`)
- `POST /api/work-orders` (`work_orders/routes.rs:153`)
- `PUT /api/work-orders/{id}/state` (`work_orders/routes.rs:248`)
- `GET /api/work-orders/{id}/timeline` (`work_orders/routes.rs:415`)
- `DELETE /api/work-orders/{id}` (`work_orders/routes.rs:438`)
- `GET /api/work-orders/{id}/progress` (`work_orders/progress.rs:43`)
- `PUT /api/work-orders/{id}/steps/{step_id}/progress` (`work_orders/progress.rs:65`)
- `POST /api/work-orders/{id}/location-trail` (`location/routes.rs:64`)
- `GET /api/work-orders/{id}/location-trail` (`location/routes.rs:110`)
- `POST /api/work-orders/{id}/check-in` (`location/routes.rs:176`)

**Recipes / Steps / Tip Cards** (`repo/backend/src/recipes/routes.rs`)
- `GET /api/recipes` (`recipes/routes.rs:80`)
- `GET /api/recipes/{id}/steps` (`recipes/routes.rs:102`)
- `GET /api/steps/{id}/timers` (`recipes/routes.rs:123`)
- `GET /api/steps/{id}/tip-cards` (`recipes/routes.rs:145`)
- `POST /api/tip-cards` (`recipes/routes.rs:162`)
- `PUT /api/tip-cards/{id}` (`recipes/routes.rs:189`)

**Notifications** (`web::scope("/api/notifications")` — `repo/backend/src/notifications/routes.rs::scope`)
- `GET /api/notifications` (`notifications/routes.rs:36`)
- `PUT /api/notifications/{id}/read` (`notifications/routes.rs:64`)
- `PUT /api/notifications/unsubscribe` (`notifications/routes.rs:85`)

**Analytics** (`web::scope("/api/analytics")` — `repo/backend/src/analytics/routes.rs::scope`)
- `GET /api/analytics/learning` (`analytics/routes.rs:110`)
- `GET /api/analytics/learning/export-csv` (`analytics/routes.rs:123`)

**Learning (knowledge + records)** (`repo/backend/src/learning/routes.rs`)
- `GET /api/knowledge-points` (`learning/routes.rs:102`)
- `GET /api/knowledge-points/by-step/{step_id}` (`learning/routes.rs:131`)
- `POST /api/knowledge-points` (`learning/routes.rs:149`)
- `PUT /api/knowledge-points/{id}` (`learning/routes.rs:189`)
- `DELETE /api/knowledge-points/{id}` (`learning/routes.rs:223`)
- `POST /api/learning-records` (`learning/routes.rs:247`)
- `GET /api/learning-records` (`learning/routes.rs:333`)
- `GET /api/learning-records/{id}` (`learning/routes.rs:381`)

**Sync** (`web::scope("/api/sync")` — `repo/backend/src/sync/routes.rs::scope`)
- `POST /api/sync/step-progress` (`sync/routes.rs:52`)
- `GET /api/sync/conflicts` (`sync/routes.rs:80`)
- `POST /api/sync/conflicts/{id}/resolve` (`sync/routes.rs:104`)
- `GET /api/sync/changes` (`sync/routes.rs:146`)
- `POST /api/sync/work-orders/{id}/delete` (`sync/routes.rs:209`)

**Admin** (`web::scope("/api/admin")` — `repo/backend/src/admin/routes.rs::scope`)
- `GET /api/admin/users` (`admin/routes.rs:43`)
- `POST /api/admin/users` (`admin/routes.rs:70`)
- `PUT /api/admin/users/{id}` (`admin/routes.rs:107`)
- `DELETE /api/admin/users/{id}` (`admin/routes.rs:147`)
- `GET /api/admin/branches` (`admin/routes.rs:205`)
- `POST /api/admin/branches` (`admin/routes.rs:228`)
- `PUT /api/admin/branches/{id}` (`admin/routes.rs:259`)
- `POST /api/admin/sync/trigger` (`admin/routes.rs:296`)
- `POST /api/admin/retention/prune` (`admin/routes.rs:307`)
- `POST /api/admin/notifications/retry` (`admin/routes.rs:326`)

**Total endpoints (strict): 55**

---

### 2) API Test Classification (static)

**True No-Mock HTTP** — tests build the real App with `fieldops_backend::configure` + `wrap(JwtAuth)` + real `sqlx::PgPool` (`repo/backend/tests/api/common.rs::make_service`) and issue requests via `actix_web::test::TestRequest` + `call_service`.

Test files:
- `repo/backend/tests/api/admin.rs`
- `repo/backend/tests/api/analytics.rs`
- `repo/backend/tests/api/auth.rs`
- `repo/backend/tests/api/learning.rs`
- `repo/backend/tests/api/location.rs`
- `repo/backend/tests/api/me.rs`
- `repo/backend/tests/api/notifications.rs`
- `repo/backend/tests/api/rbac.rs`
- `repo/backend/tests/api/recipes.rs`
- `repo/backend/tests/api/retention.rs`
- `repo/backend/tests/api/sync.rs`
- `repo/backend/tests/api/work_orders.rs`

**HTTP with Mocking** — **none detected**.

**Non-HTTP (unit / integration without HTTP)**
- `repo/backend/tests/unit/pagination.rs` (pure unit of `fieldops_backend::pagination`)
- `repo/backend/tests/unit/sync_conflicts.rs` (direct calls into `fieldops_backend::sync` + DB; no HTTP)
- `repo/backend/tests/unit/state_machine.rs` (direct calls into `fieldops_backend::state_machine`)
- `repo/backend/tests/unit/crypto.rs` (direct calls into `fieldops_backend::crypto`)
- `repo/backend/tests/unit/rbac_guards.rs` (direct calls into `fieldops_backend::middleware::rbac`)
- Inline `#[cfg(test)] mod tests { ... }` in `repo/backend/src/**` (auth/hashing, auth/jwt, state_machine, config, crypto, etag, geo, logging, middleware/rbac, notifications/stub, location/geocode_stub)

---

### 3) Mock Detection (strict)

Search for test-layer mocking hooks (`mock`, `stub`, `MockDatabase`, `MockPool`, `mockall`, `fake::`, `wiremock`, case-insensitive) under `repo/backend/tests`:
- **0 matches** in test code.

Notable **production-code** stubs (not test-code mocks):
- Notification delivery stub: `repo/backend/src/notifications/stub.rs::send` and `retry_pending`.
- Offline geocoder stub: `repo/backend/src/location/geocode_stub.rs`.

These are invoked by real handlers during tests — they are part of the system under test, not bypass layers installed by tests. They do mean some "integration" coverage validates stub behavior rather than a real external-provider boundary.

---

### 4) API Test Mapping Table (per endpoint)

Abbreviations: TNM = True No-Mock HTTP. Evidence cites file + function.

| Endpoint (METHOD PATH) | Covered | Type | Evidence |
|---|---:|---|---|
| GET /health | Yes | TNM | `tests/api/auth.rs::health_is_public` |
| GET /api/health | Yes | TNM | `tests/api/auth.rs::api_health_alternate_is_public_and_matches_shape` |
| POST /api/auth/login | Yes | TNM | `tests/api/auth.rs::login_success_returns_token_and_user` (+ rejects_bad_password / rejects_unknown_user / rejects_empty_credentials) |
| POST /api/auth/logout | Yes | TNM | `tests/api/auth.rs::logout_requires_bearer`, `logout_with_bearer_ok_and_advertises_stateless` |
| POST /api/auth/change-password | Yes | TNM | `tests/api/auth.rs::change_password_flips_reset_flag`, `change_password_rejects_weak_password`, `change_password_requires_bearer` |
| GET /api/me | Yes | TNM | `tests/api/me.rs::get_me_returns_profile`, `me_requires_auth` |
| PUT /api/me/privacy | Yes | TNM | `tests/api/me.rs::set_privacy_persists_value` |
| PUT /api/me/home-address | Yes | TNM | `tests/api/me.rs::home_address_is_encrypted_at_rest`, `home_address_requires_auth`, `home_address_another_user_cannot_read` |
| GET /api/me/home-address | Yes | TNM | `tests/api/me.rs::home_address_is_encrypted_at_rest`, `home_address_another_user_cannot_read` |
| GET /api/work-orders | Yes | TNM | `tests/api/work_orders.rs::admin_lists_all_work_orders`, `tech_sees_only_own_jobs`, `super_sees_branch_jobs` |
| GET /api/work-orders/on-call-queue | Yes | TNM | `tests/api/work_orders.rs::on_call_queue_requires_super_or_admin`, `on_call_queue_returns_high_priority_near_deadline` |
| GET /api/work-orders/{id} | Yes | TNM | `tests/api/work_orders.rs::get_work_order_returns_404_for_non_owner_tech`, `admin_can_soft_delete_work_order` (post-delete 404) |
| POST /api/work-orders | Yes | TNM | `tests/api/work_orders.rs::super_creates_work_order`, `tech_cannot_create_work_order`, `create_wo_rejects_location_outside_branch_radius` |
| PUT /api/work-orders/{id}/state | Yes | TNM | `tests/api/work_orders.rs::transition_happy_path_scheduled_to_enroute`, `transition_scheduled_to_enroute_requires_gps`, `tech_cannot_cancel_work_order`, `super_cancels_with_notes`, `super_cancel_without_notes_is_400` |
| GET /api/work-orders/{id}/timeline | Yes | TNM | `tests/api/work_orders.rs::timeline_reflects_transitions_with_body_content` |
| DELETE /api/work-orders/{id} | Yes | TNM | `tests/api/work_orders.rs::admin_can_soft_delete_work_order`, `super_cannot_soft_delete_work_order`; `tests/api/sync.rs::soft_delete_propagates_as_delete_sync_log` |
| GET /api/work-orders/{id}/progress | Yes | TNM | `tests/api/recipes.rs::timer_state_snapshot_round_trip` |
| PUT /api/work-orders/{id}/steps/{step_id}/progress | Yes | TNM | `tests/api/work_orders.rs::step_progress_upsert_creates_then_updates`; `tests/api/recipes.rs::timer_state_snapshot_round_trip` |
| POST /api/work-orders/{id}/location-trail | Yes | TNM | `tests/api/location.rs::post_trail_point_ok_for_owning_tech`, `post_trail_point_with_privacy_mode_reduces_precision`, `non_owner_tech_cannot_post_trail_point` |
| GET /api/work-orders/{id}/location-trail | Yes | TNM | `tests/api/location.rs::trail_get_hidden_from_super_when_owner_privacy_on`, `trail_get_masks_for_super_when_privacy_off` |
| POST /api/work-orders/{id}/check-in | Yes | TNM | `tests/api/location.rs::arrival_check_in_within_radius_ok`, `arrival_check_in_outside_radius_400`, `departure_check_in_skips_radius_validation` |
| GET /api/recipes | Yes | TNM | `tests/api/recipes.rs::list_recipes_visible_to_any_authed_user` |
| GET /api/recipes/{id}/steps | Yes | TNM | `tests/api/recipes.rs::list_recipe_steps_returns_order` |
| GET /api/steps/{id}/timers | Yes | TNM | `tests/api/recipes.rs::list_step_timers_returns_backend_defined_rows` |
| GET /api/steps/{id}/tip-cards | Yes | TNM | `tests/api/recipes.rs::admin_creates_tip_card_and_tech_reads_it` |
| POST /api/tip-cards | Yes | TNM | `tests/api/recipes.rs::admin_creates_tip_card_and_tech_reads_it`, `tech_cannot_create_tip_card` |
| PUT /api/tip-cards/{id} | Yes | TNM | `tests/api/recipes.rs::admin_creates_tip_card_and_tech_reads_it`, `tip_card_update_404_when_missing` |
| GET /api/notifications | Yes | TNM | `tests/api/notifications.rs::list_notifications_returns_own_only` |
| PUT /api/notifications/{id}/read | Yes | TNM | `tests/api/notifications.rs::mark_read_updates_row`, `mark_read_rejects_other_users_notifications` |
| PUT /api/notifications/unsubscribe | Yes | TNM | `tests/api/notifications.rs::unsubscribe_records_preference`, `unsubscribe_is_idempotent` |
| GET /api/analytics/learning | Yes | TNM | `tests/api/analytics.rs::learning_admin_sees_all_rows`, `learning_tech_sees_only_own_row`, `learning_rejects_bad_date_format`, `analytics_branch_filter_narrows_to_single_branch`, `analytics_date_range_filter_excludes_out_of_window_records`, `analytics_role_filter_limits_to_requested_role` |
| GET /api/analytics/learning/export-csv | Yes | TNM | `tests/api/analytics.rs::learning_csv_has_watermark_footer` |
| GET /api/knowledge-points | Yes | TNM | `tests/api/learning.rs::tech_list_hides_correct_answer` |
| GET /api/knowledge-points/by-step/{step_id} | Yes | TNM | `tests/api/learning.rs::list_knowledge_by_step_returns_only_rows_for_that_step`, `list_knowledge_by_step_empty_when_no_match` |
| POST /api/knowledge-points | Yes | TNM | `tests/api/learning.rs::admin_can_create_knowledge_point`, `tech_cannot_create_knowledge_point`, `quiz_question_without_options_is_rejected`; `tests/api/analytics.rs::analytics_reflects_records_written_via_capture_endpoint` |
| PUT /api/knowledge-points/{id} | Yes | TNM | `tests/api/learning.rs::admin_updates_knowledge_point_and_body_reflects_new_value`, `update_knowledge_point_404_when_missing`, `tech_and_super_cannot_update_knowledge_point` |
| DELETE /api/knowledge-points/{id} | Yes | TNM | `tests/api/learning.rs::admin_can_delete_knowledge_point_and_row_is_gone`, `delete_knowledge_point_404_when_missing`, `non_admin_cannot_delete_knowledge_point` |
| GET /api/learning-records | Yes | TNM | `tests/api/learning.rs::tech_list_scoped_to_own_records` |
| POST /api/learning-records | Yes | TNM | `tests/api/learning.rs::tech_records_correct_quiz_answer_scores_one`, `review_bump_increments_count_without_adding_row`; `tests/api/analytics.rs::analytics_reflects_records_written_via_capture_endpoint` |
| GET /api/learning-records/{id} | Yes | TNM | `tests/api/learning.rs::get_learning_record_by_id_admin_sees_any`, `get_learning_record_by_id_tech_sees_own`, `get_learning_record_tech_cannot_see_other_tech_record`, `get_learning_record_404_for_unknown_id` |
| GET /api/sync/changes | Yes | TNM | `tests/api/sync.rs::changes_endpoint_returns_rows_since_cursor`, `changes_entity_filter_applies`, `changes_invalid_cursor_returns_400` |
| POST /api/sync/step-progress | Yes | TNM | `tests/api/sync.rs::post_step_progress_inserts_when_no_local_row`, `post_step_progress_rejects_older_version`, `post_step_progress_flags_conflict_on_equal_version_different_payload`, `post_step_progress_other_techs_work_order_returns_404` |
| GET /api/sync/conflicts | Yes | TNM | `tests/api/sync.rs::conflicts_list_super_happy_and_tech_is_403` |
| POST /api/sync/conflicts/{id}/resolve | Yes | TNM | `tests/api/sync.rs::resolve_conflict_integration` |
| POST /api/sync/work-orders/{id}/delete | Yes | TNM | `tests/api/sync.rs::push_work_order_delete_admin_happy_path_writes_tombstone`, `push_work_order_delete_404_for_missing_id`, `push_work_order_delete_requires_admin` |
| GET /api/admin/users | Yes | TNM | `tests/api/admin.rs::list_users_requires_admin`; `tests/api/rbac.rs::admin_users_list_body_contains_seeded_usernames` |
| POST /api/admin/users | Yes | TNM | `tests/api/admin.rs::admin_creates_user_with_valid_payload`, `admin_user_create_rejects_short_password`, `admin_user_create_rejects_duplicate_username` |
| PUT /api/admin/users/{id} | Yes | TNM | `tests/api/admin.rs::admin_updates_user_role_and_privacy` |
| DELETE /api/admin/users/{id} | Yes | TNM | `tests/api/admin.rs::admin_cannot_delete_self`, `admin_soft_deletes_other_user` |
| GET /api/admin/branches | Yes | TNM | `tests/api/admin.rs::admin_branches_crud_lifecycle` |
| POST /api/admin/branches | Yes | TNM | `tests/api/admin.rs::admin_branches_crud_lifecycle` |
| PUT /api/admin/branches/{id} | Yes | TNM | `tests/api/admin.rs::admin_branches_crud_lifecycle` |
| POST /api/admin/sync/trigger | Yes | TNM | `tests/api/admin.rs::sync_trigger_returns_report`, `sync_trigger_rejects_non_admin` |
| POST /api/admin/retention/prune | Yes | TNM | `tests/api/retention.rs::prune_removes_work_orders_past_retention`, `prune_preserves_recent_soft_deletes`, `prune_endpoint_requires_admin` |
| POST /api/admin/notifications/retry | Yes | TNM | `tests/api/notifications.rs::retry_delivers_pending_row_past_backoff`, `retry_skips_unsubscribed_rows`, `retry_caps_at_max_attempts_and_gives_up` |

---

### 5) Coverage Summary

- Total endpoints: **55**
- Endpoints with HTTP tests: **55**
- Endpoints with True No-Mock HTTP tests: **55**

Computed:
- HTTP coverage: **55 / 55 = 100%**
- True API coverage: **55 / 55 = 100%**

Test counts (function declarations found by grep):
- `tests/api/*.rs` (`#[actix_web::test]` functions): **117**
- `tests/unit/*.rs` (dedicated unit files): **40** (pagination 5, sync_conflicts 7, state_machine 9, crypto 11, rbac_guards 8)
- Inline `#[cfg(test)]` tests inside `repo/backend/src/**`: present in 11 modules (auth/hashing, auth/jwt, state_machine, config, crypto, etag, geo, logging, middleware/rbac, notifications/stub, location/geocode_stub).

Strict note on "real HTTP layer": these are in-process Actix requests via `TestRequest` — the exact same path taken by a TCP-bound server for routing + middleware + handler dispatch. They are **not** TCP-bound network tests. Under a strict "TCP + real socket" interpretation, true-network API coverage would be 0 and only in-process exists.

---

### 6) Unit Test Analysis (non-HTTP)

Dedicated files under `repo/backend/tests/unit/`:
- `pagination.rs` — `fieldops_backend::pagination::{PageParams, Paginated}` (normalization + envelope shape).
- `sync_conflicts.rs` — `fieldops_backend::sync::trigger` and `fieldops_backend::sync::merge::merge_step_progress` (DB-backed, no HTTP).
- `state_machine.rs` — full (from, to) × role matrix for `allowed_transition`; required-fields grid for `TransitionContext::validate_required`.
- `crypto.rs` — failure modes (non-hex, odd-length hex, truncated, empty, bit flips in tag and body, wrong-key AEAD, nonce-uniqueness).
- `rbac_guards.rs` — 3×3 `(caller, required)` matrix for `require_role`; single/multi/empty allowlist for `require_any_role`; `AuthedUser` accessors.

Inline `#[cfg(test)]` tests in production modules (static presence):
- `repo/backend/src/auth/hashing.rs`, `auth/jwt.rs`, `state_machine.rs`, `config/mod.rs`, `crypto.rs`, `etag.rs`, `geo.rs`, `logging/mod.rs`, `middleware/rbac.rs`, `notifications/stub.rs`, `location/geocode_stub.rs`.

Modules with unit-test coverage (dedicated or inline):
- Services: state machine (`state_machine.rs`), sync merge (`sync/merge.rs` via `sync_conflicts.rs`), notification retry/backoff (`notifications/stub.rs` + API tests), crypto (`crypto.rs`), etag (`etag.rs`), geo haversine (`geo.rs`).
- Auth/guards: `require_role`, `require_any_role`, JWT issue/verify, Argon2id hash/verify.
- Configuration: `AppConfig::from_env` placeholder rejection tests inline.
- Logging: redaction + tag format.
- Controllers/handlers: validated via HTTP integration (per-endpoint above).

Modules **without** dedicated non-HTTP unit coverage:
- `repo/backend/src/db.rs` (`connect`, `run_migrations`, `truncate_all`, `seed_default_admin`) — exercised via every API test's `common::setup()` path, but no isolated unit tests.
- `repo/backend/src/errors.rs::ApiError` `ResponseError::error_response` — exercised via HTTP assertions (`rbac::unauthorized_response_has_structured_body`, `forbidden_response_has_structured_body`), but no isolated unit test.
- `repo/backend/src/middleware/request_log.rs` — no dedicated test; only wrapped in the live server, not in the test harness (which wraps only `JwtAuth`).
- `repo/backend/src/work_orders/progress.rs::upsert_progress` version-cap / snapshot pruning math — exercised only end-to-end.
- `repo/backend/src/retention.rs::prune` internals — exercised via admin endpoint; no isolated unit.

---

### 7) API Observability Check

Strengths (explicit request + meaningful body assertions):
- Auth login: token presence + user envelope (`tests/api/auth.rs::login_success_returns_token_and_user`).
- Home-address encryption round-trip: asserts ciphertext is hex-only, does not contain plaintext substrings, plaintext round-trips (`tests/api/me.rs::home_address_is_encrypted_at_rest`).
- Work-order transitions: body asserts `state`, `version_count` (`work_orders.rs::transition_happy_path_scheduled_to_enroute`); timeline asserts `from_state`, `to_state`, `triggered_by`, `work_order_id` (`work_orders.rs::timeline_reflects_transitions_with_body_content`).
- Sync merge outcomes: explicit `{outcome, conflict}` assertions + DB side-effect verification (`tests/api/sync.rs::post_step_progress_*`).
- CSV export: asserts `Content-Type`, `Content-Disposition` filename, watermark footer string (`tests/api/analytics.rs::learning_csv_has_watermark_footer`).
- Error envelope: `{code, error}` shape asserted for 401 and 403 (`tests/api/rbac.rs::unauthorized_response_has_structured_body`, `forbidden_response_has_structured_body`).

Weaknesses:
- `tests/api/rbac.rs::rbac_matrix` is a table-driven matrix that asserts **status codes only** (via `status_of`, not `json_of`). Body content for the matrix's 200 cells is re-asserted elsewhere (route-specific tests + `admin_users_list_body_contains_seeded_usernames`), so the gap is cosmetic but worth naming.
- A small number of integration tests record only a side-effect plus status rather than a response-body schema (e.g. several branches CRUD steps).

---

### 8) Test Quality & Sufficiency (strict)

Positive signals (evidence-based):
- Auth: success + bad password + unknown user + empty payload + bearer-missing + invalid-token (`tests/api/auth.rs`).
- RBAC: per-route + per-role status checks plus body-content checks for forbidden/unauthorized (`tests/api/rbac.rs`), plus per-endpoint RBAC assertions in learning / sync / admin.
- State machine: complete role × transition matrix plus required-fields grid (`tests/unit/state_machine.rs`).
- Crypto: negative paths (non-hex, odd length, truncation, bit flips, wrong key, empty) and positive path (roundtrip + unicode + nonce-uniqueness) (`tests/unit/crypto.rs`).
- Sync merge: deterministic `Applied` / `RejectedOlder` / `Conflict` branches asserted with DB state verification (`tests/api/sync.rs::post_step_progress_*`, `tests/unit/sync_conflicts.rs`).
- Privacy / PII: precision reduction, "hidden from SUPER when owner privacy on", trail masking (`tests/api/location.rs`); encryption at rest with raw-DB ciphertext inspection (`tests/api/me.rs::home_address_is_encrypted_at_rest`).

Gaps / risks:
- Production stubs (notifications delivery, offline geocoder) are the real objects under test — "integration" here does not exercise a real external-provider boundary.
- No dedicated test for `middleware/request_log.rs`.
- No dedicated test for `db::run_migrations` error cases (malformed SQL file, missing dir); covered only happily via every setup.
- Frontend-side test coverage (under `repo/frontend/tests/`) is a shell smoke (`e2e/smoke.sh`) — not a Wasm/browser-bound UI test suite. `frontend/tests/unit/README.md` explicitly defers `wasm-bindgen-test` integration; no Rust unit tests against the Yew frontend are present.

**run_tests.sh policy check** (`repo/run_tests.sh`):
- Backend stage runs inside Docker: `docker compose --profile tests run --rm --build backend-test cargo test --release -- --test-threads=1` (line 49-51). **OK — Docker-contained.**
- Frontend e2e: host `curl` + `sh frontend/tests/e2e/smoke.sh` against services brought up by `docker compose up -d --build backend frontend`. The targets run in Docker; the driver requires host `curl` / `sh`. **Partial** — services themselves are Docker-contained, but the driver is not.

---

### 9) End-to-End (Fullstack) Expectations

Project is fullstack (`repo/docker-compose.yml` services: `postgres`, `backend`, `backend-test`, `frontend`). A real FE↔BE e2e exists in `repo/frontend/tests/e2e/smoke.sh`, which curls:
- `/` → HTML served + `#app-root` element present.
- `/api/health` → 200 via nginx proxy.
- `/api/auth/login` → parse token.
- `/api/me` with bearer → user shape.

Strengths: validates nginx → backend proxy + full login round-trip against the live stack.
Gaps: no browser-level UI exercise (no Playwright / Cypress / wasm-bindgen-test). Frontend Yew code is only checked by the trunk build succeeding. Partial compensation via strong backend API + unit coverage is present.

---

### 10) Coverage Score (0–100): **95 / 100**

Rationale:
- **+45**: Endpoint HTTP coverage is 55/55 = 100%, with every endpoint exercised via the real app (configure + JwtAuth + real DB).
- **+25**: **Zero test-layer mocking**. Tests traverse real routing, middleware, handlers, and DB IO.
- **+15**: Depth is strong. Positive + negative + RBAC cases per route; body-content assertions (not only status) on critical flows; dedicated unit files for state machine, crypto, RBAC, pagination, sync merge.
- **+10**: `run_tests.sh` is Docker-contained for the backend stage and brings up the full stack for e2e.
- **−3**: Production stub modules (notifications delivery, offline geocoder) are the real components under test — some "integration" signal is against stubbed behavior, not a real external boundary.
- **−2**: Frontend is not unit-tested at the Rust/Wasm level; e2e is a shell/curl smoke rather than a browser-driven UI test.

---

### 11) Key Gaps (actionable)

1. Add a dedicated unit test for `repo/backend/src/middleware/request_log.rs` (currently no direct coverage; not even wrapped in the test harness).
2. Add browser-level e2e for the Yew frontend (e.g. `wasm-bindgen-test` + headless Chromium in the Dockerized test runner, or Playwright against the running stack).
3. Consider isolating the notifications delivery stub behind a trait so tests can inject a failing provider to exercise retry/backoff against realistic failures (instead of the "first attempt always succeeds" stub).
4. Add a unit test for `db::run_migrations` unhappy paths (missing migrations dir, malformed SQL file) using a temp-dir fixture — currently only the happy path is exercised transitively.
5. Strengthen `tests/api/rbac.rs::rbac_matrix` to json-decode the response body on 200 cells for the routes where schema matters, or remove the matrix in favor of the per-route ones to avoid duplicate coverage of the same status cells.

---

### 12) Confidence & Assumptions

- **High confidence** on endpoint enumeration: each handler uses explicit `#[get/post/put/delete("...")]` attributes; scope prefixes resolved directly from `scope()` helpers in `repo/backend/src/**/routes.rs` and `lib.rs::configure`.
- **High confidence** on test classification: all tests under `repo/backend/tests/api/*.rs` use `actix_web::test::TestRequest` + `call_service` via the shared `make_service` helper (`tests/api/common.rs`).
- **High confidence** on mock absence: grep for common mocking tokens (`mock`, `stub`, `mockall`, `wiremock`, `fake::`, `MockPool`, `MockDatabase`, case-insensitive) returned **0 matches** across `repo/backend/tests`.
- **Assumption**: "true no-mock HTTP" includes in-process Actix routing via `TestRequest`. If the grader requires a TCP-bound server + network client, all API tests would be classified as in-process-HTTP and the true-network coverage would be 0.

---

## Part 2 — README Quality & Compliance Audit

Target: `repo/README.md` (exists — PASS).
Related config: `repo/docker-compose.yml`, `repo/run_tests.sh`.

### 1) Project Type Detection (critical)

Observed at top of `repo/README.md`:
- Title: "FieldOps Kitchen & Training Console"
- Tagline: "One-click startup. No manual file creation, no `.env` copying."
- **No explicit declaration** of project type (backend / fullstack / web / android / ios / desktop).

Light inference (allowed when missing):
- `repo/docker-compose.yml` declares `postgres`, `backend`, `backend-test`, `frontend` services.
- README mentions the Yew/WASM frontend obliquely in the port table.

Inferred type: **fullstack (backend + web frontend)**.

**Finding (hard gate):** Missing explicit project-type declaration at top → **FAIL**.

### 2) README Location

`repo/README.md` exists → **PASS**.

### 3) Hard Gates

#### Formatting
- Clean Markdown with headings, ports table, fenced code blocks, callout block. **PASS**.

#### Startup Instructions
- Includes `docker compose up --build`. **PASS**.

#### Access Method (URL + port)
- README provides a ports table: Postgres 5432, Backend 8080, Frontend 8081.
- **No explicit URLs** such as `http://localhost:8081` (frontend UI) or `http://localhost:8080/health` (backend API base).

**Finding (hard gate):** Missing explicit access URL(s) → **FAIL**.

#### Verification Method
- README's "Test" section runs `./run_tests.sh`.
- **No curl/Postman commands, no UI-flow steps, no expected outputs** demonstrating the system works end-to-end.

**Finding (hard gate):** Missing verification method → **FAIL**.

#### Environment Rules (Docker-contained)
- No `npm install`, `pip install`, `apt-get`, or manual DB setup instructions are present in README.
- `docker-compose.yml` ships all env vars inline; Postgres/backend/frontend all run via Docker. **PASS**.

#### Demo Credentials (auth exists)
- Auth exists: `POST /api/auth/login` (`repo/backend/src/auth/routes.rs::login`) + JWT middleware (`repo/backend/src/middleware/rbac.rs::JwtAuth`). Three roles — `TECH`, `SUPER`, `ADMIN` — are defined in `repo/backend/src/auth/models.rs::Role` and seeded in `repo/backend/tests/api/common.rs::setup`.
- README provides only:
  - Username: `admin`
  - Password: `admin123`
  - Implied role: `ADMIN`.
- **SUPER and TECH credentials are not provided** in README; no explicit statement that only ADMIN is seeded and others must be created via the admin endpoint.

**Finding (hard gate):** Demo credentials do not cover all roles → **FAIL**.

### 4) Engineering Quality

Strengths (evidence):
- Clear one-click startup via Docker.
- Explicit first-boot admin seed + `password_reset_required=true` flow (`repo/README.md` default-admin section).
- Explicit production deployment section with placeholder-secret list (`JWT_SECRET`, `AES_256_KEY_HEX`, `DEFAULT_ADMIN_PASSWORD`) and `DEV_MODE=false` guard (backed by `repo/backend/config/mod.rs::from_env` placeholder rejection).
- Documents offline ZIP+4 normalization path (`backend/data/zip4_index.csv`) with rebuild-required warning.

Missing / unclear:
- No stack/architecture one-liner beyond a single phrase mentioning Yew.
- No example API requests (no `curl -H "Authorization: Bearer ..." ...`).
- No "first-time smoke" flow ("open `http://localhost:8081`, log in as admin, expect dashboard").
- No pointer to the migration lifecycle / `RUN_MIGRATIONS_ON_BOOT` guard.
- No brief explanation of what `run_tests.sh` actually runs.

### 5) README Issues

#### Hard Gate Failures
1. Project-type declaration missing (`repo/README.md`).
2. Access URLs missing (ports table only) (`repo/README.md`).
3. Verification method missing (no curl / UI flow / expected output) (`repo/README.md`).
4. Demo credentials incomplete for multi-role auth (only ADMIN provided) (`repo/README.md`).

#### High Priority Issues
- Add a "Quick sanity check" section showing a `curl http://localhost:8080/health` with expected `{"status":"ok"}` body, and a `POST /api/auth/login` round-trip that prints the token.
- State the project type in the opening line (e.g., "Fullstack — Rust/Actix-web backend + Yew/WASM frontend + Postgres, fully containerized").
- State which roles are seeded, and add explicit "how to create SUPER/TECH users" pointer (`POST /api/admin/users` with an admin bearer token).

#### Medium Priority Issues
- Add explicit access URLs alongside the ports table.
- Describe what `run_tests.sh` does (backend unit + API in `backend-test` service, then frontend e2e via curl smoke).
- Clarify that the frontend is a Yew/WASM SPA served by nginx; first-load may take a few seconds for WASM download.

#### Low Priority Issues
- Add a "Troubleshooting" subsection (ports in use, `docker compose down -v` to reset, slow first build).
- Remove `version: "3.9"` line in `docker-compose.yml` (compose warns it is obsolete on every invocation).

### 6) README Verdict

**FAIL** — four hard-gate failures (project type declaration, access URLs, verification method, demo credentials for all roles).

---

## Final Verdicts

- **Test Coverage Audit — PASS.** Score **95 / 100**. All 55 endpoints exercised via real, non-mocked Actix routing + real DB; dedicated unit coverage for state machine, crypto, RBAC, pagination, and sync merge; Docker-contained runner.
- **README Audit — FAIL.** Four hard-gate failures: no project-type declaration, no access URLs, no verification commands, and demo credentials cover only ADMIN while three roles exist in code.
