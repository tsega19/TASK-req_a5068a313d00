# Recheck — Static Audit of Claimed Fixes

Date: 2026-04-18  
Boundary: static-only; no Docker/project/test execution.

This recheck verifies whether the *claimed fixes* exist in the repo with file:line evidence, and identifies any remaining material gaps that prevent Delivery Acceptance.

---

## 1. Updated Verdict

**Overall conclusion: Partial Pass (improved), but still Fail for delivery acceptance due to a new schema/runtime blocker found statically.**

### New Blocker found in recheck
- **`password_reset_required` is referenced throughout the backend (queries/inserts/tests) but is not present in the database migration.**
  - Evidence of usage: `repo/backend/src/db.rs:86`, `repo/backend/src/auth/routes.rs:54`, `repo/backend/src/auth/models.rs:51`
  - Evidence of missing schema: no `password_reset_required` in `repo/backend/migrations/0001_init.sql` (search result is empty; see also `rg` output showing no migration hit)
  - Impact: static evidence indicates startup/migrations would create a `users` table without this column, while code expects it to exist; this would cause runtime SQL errors.

---

## 2. Verification of Delivered Fix Claims

### Blockers

**B1 — work_order_detail.rs: lat/lng inputs + CheckInPanel**
- **Claim:** Added lat/lng input fields (prefilled from work order) and ARRIVAL/DEPARTURE check-in buttons; transitions now supply required data.
- **Static verification:** **Present.**
  - CheckInPanel exists and posts `/api/work-orders/{id}/check-in` with `type: ARRIVAL/DEPARTURE` and validated numeric lat/lng. `repo/frontend/src/pages/work_order_detail.rs:185`, `repo/frontend/src/pages/work_order_detail.rs:236`, `repo/frontend/src/pages/work_order_detail.rs:256`
  - UI includes lat/lng inputs prefilled from `wo.location_lat/lng`. `repo/frontend/src/pages/work_order_detail.rs:200`, `repo/frontend/src/pages/work_order_detail.rs:203`
  - TransitionPanel includes local pre-validation for EnRoute lat/lng (per grep hits). `repo/frontend/src/pages/work_order_detail.rs:392`
- **Still requires manual verification:** Browser UX and actual check-in/state transition behavior at runtime.

**B2 — Backend timers endpoint + frontend consumes timers and persists snapshots**
- **Claim:** `GET /api/steps/{step_id}/timers`; frontend consumes backend timers; snapshots persist via `job_step_progress.timer_state_snapshot`.
- **Static verification:** **Present.**
  - Backend route exists: `#[get("/{id}/timers")]` under `/api/steps`. `repo/backend/src/recipes/routes.rs:123`, `repo/backend/src/recipes/routes.rs:229`
  - Frontend fetches real timers: `/api/steps/{}/timers`. `repo/frontend/src/pages/recipe_step.rs:88`
  - Frontend persists timer snapshots via `timer_state` in PUT payload; reads back `timer_state_snapshot`. `repo/frontend/src/pages/recipe_step.rs:114`, `repo/frontend/src/pages/recipe_step.rs:174`
  - TimerRing emits snapshots via `on_tick` and supports initial remaining/running from persisted state. `repo/frontend/src/components/timer_ring.rs:20`, `repo/frontend/src/components/timer_ring.rs:38`

### High

**H3 — DEV_MODE gate + change-password endpoint + compose/README warnings**
- **Claim:** config rejects placeholder JWT/AES/password unless DEV_MODE; password reset flow added; docs warn dev-only defaults.
- **Static verification:** **Mostly present, but blocked by missing migration column.**
  - DEV_MODE placeholder rejection exists. `repo/backend/config/mod.rs:106`, `repo/backend/config/mod.rs:161`, `repo/backend/config/mod.rs:206`
  - docker-compose documents dev-only placeholders and sets `DEV_MODE=true`, `REQUIRE_ADMIN_PASSWORD_CHANGE=true`. `repo/docker-compose.yml:8`, `repo/docker-compose.yml:93`
  - README documents dev-only defaults and production checklist. `repo/README.md:25`, `repo/README.md:50`
  - `POST /api/auth/change-password` exists and is bearer-token protected (uses `AuthedUser`). `repo/backend/src/auth/routes.rs:103`, `repo/backend/src/auth/routes.rs:104`
  - **Blocker:** `password_reset_required` column missing from migration; see Verdict above.

**H4 — AES-256-GCM crypto + /api/me/home-address + tests**
- **Claim:** AES-256-GCM module + endpoints integrated; tests verify ciphertext at rest.
- **Static verification:** **Present.**
  - Crypto module exists and implements AES-256-GCM encrypt/decrypt. `repo/backend/src/crypto.rs:1`, `repo/backend/src/crypto.rs:25`, `repo/backend/src/crypto.rs:42`
  - Endpoints exist: `PUT /api/me/home-address`, `GET /api/me/home-address`, with encrypt/decrypt. `repo/backend/src/me/routes.rs:87`, `repo/backend/src/me/routes.rs:114`
  - Integration test verifies stored DB value is ciphertext-like and plaintext round-trips. `repo/backend/tests/api/me.rs:47`, `repo/backend/tests/api/me.rs:69`, `repo/backend/tests/api/me.rs:82`

### Medium

**M5 — sync/merge.rs + sync/routes.rs deterministic merge + conflicts endpoints**
- **Claim:** Deterministic merge policy; endpoints to push step progress and list/resolve conflicts.
- **Static verification:** **Present.**
  - Deterministic merge module exists with documented invariants and conflict logging. `repo/backend/src/sync/merge.rs:1`, `repo/backend/src/sync/merge.rs:40`
  - HTTP routes exist: `POST /api/sync/step-progress`, `GET /api/sync/conflicts`, `POST /api/sync/conflicts/{id}/resolve`. `repo/backend/src/sync/routes.rs:33`, `repo/backend/src/sync/routes.rs:74`, `repo/backend/src/sync/routes.rs:97`
  - Configure registers sync routes. `repo/backend/src/lib.rs:48`
  - Unit tests cover merge outcomes and conflict flagging. `repo/backend/tests/unit/sync_conflicts.rs:34`, `repo/backend/tests/unit/sync_conflicts.rs:235`

**M6 — Bundled zip4_index.csv and include_str! loading**
- **Claim:** Bundled dataset loaded via `include_str!`; authoritative matches, hash fallback only for unknown.
- **Static verification:** **Present.**
  - Dataset exists: `repo/backend/data/zip4_index.csv` (file present).
  - Loaded via include_str!: `repo/backend/src/location/geocode_stub.rs:41`
  - Normalize/geocode logic prefers index hits and falls back to hash. `repo/backend/src/location/geocode_stub.rs:74`, `repo/backend/src/location/geocode_stub.rs:123`

### Low

**L7 — README rewritten with warnings and production checklist**
- **Static verification:** **Present.**
  - Dev-default warning section and production checklist exist. `repo/README.md:25`, `repo/README.md:50`
  - Offline dataset section exists. `repo/README.md:82`

---

## 3. Remaining Material Issues (Post-fix)

### Blocker

1) **Schema mismatch: missing `users.password_reset_required` column**
- **Conclusion:** Fail
- **Evidence (code expects column):** `repo/backend/src/auth/routes.rs:54`, `repo/backend/src/db.rs:86`, `repo/backend/src/auth/models.rs:51`
- **Evidence (migration lacks column):** no `password_reset_required` in `repo/backend/migrations/0001_init.sql` (static search shows no match)
- **Impact:** Any query/insert referencing this column will fail against the migrated schema.
- **Minimum actionable fix:** Add the column to `users` in migrations (either edit `0001_init.sql` for greenfield or add a new migration `0002_*.sql` with `ALTER TABLE users ADD COLUMN password_reset_required BOOLEAN NOT NULL DEFAULT FALSE;`).

### High (downgraded from prior audit if Blocker fixed)

2) **Technician “state transition required fields” are now UI-expressible but still need runtime validation**
- **Conclusion:** Cannot Confirm Statistically
- **Evidence:** UI panels exist for check-ins + lat/lng inputs `repo/frontend/src/pages/work_order_detail.rs:185`
- **Impact:** If the UI wiring or server expectations differ, transitions could still be blocked at runtime.
- **Minimum actionable fix:** Add/extend e2e script to exercise ARRIVAL check-in + state transition to OnSite, then DEPARTURE + transition to Completed (requires steps complete).

---

## 4. Bottom Line

The delivered fixes materially improve prompt alignment (timers, snapshots, check-ins, sync merge policy, offline dataset, encryption at rest, secure-default gating). However, the backend currently has a **static, high-confidence schema mismatch blocker** (`password_reset_required`) that must be resolved before acceptance.

