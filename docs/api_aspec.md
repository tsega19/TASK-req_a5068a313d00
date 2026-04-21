# FieldOps Kitchen & Training Console - API Specification

FieldOps is a client/server application. This document is the contract between the Yew/WASM frontend and the Rust/Actix backend.

- Transport: HTTP/JSON over `/api/*`, fronted by nginx.
- Auth: `Authorization: Bearer <JWT>` on all non-auth endpoints.
- Source of truth for DB types: [repo/backend/migrations/0001_init.sql](repo/backend/migrations/0001_init.sql) and [repo/backend/src/enums.rs](repo/backend/src/enums.rs).

## 1) Data model (domain types)

### 1.1 Users and session

- `Role`: `'TECH' | 'SUPER' | 'ADMIN'`
- `UserRecord`
  - `username` is unique (DB unique index).
  - `password_hash` is an Argon2 hash (never returned from the API).
  - `home_address_enc` is AES-256-GCM ciphertext (12-byte nonce + ciphertext + auth tag, hex-encoded).
  - `privacy_mode: bool` — when on, responses coarsen GPS precision (see `geo.rs`).
  - `password_reset_required: bool` — set on seed and on admin reset.
- `Session` is represented by a JWT claim set:
  - `sub` (user_id), `username`, `role`, `branch_id`, `iat`, `exp`.

### 1.2 Branches

- `BranchRecord`: `name`, `address`, `lat`, `lng`, `service_radius_miles`.

### 1.3 Work orders

- `WorkOrderRecord`
  - `priority: 'LOW' | 'NORMAL' | 'HIGH' | 'CRITICAL'`
  - `state`: one of `'Scheduled' | 'EnRoute' | 'OnSite' | 'InProgress' | 'WaitingOnParts' | 'Completed' | 'Canceled'`
    (source of truth: [repo/backend/src/enums.rs](repo/backend/src/enums.rs)). `Completed` and
    `Canceled` are the terminal states; all others are live.
  - Transitions are strictly role-gated by [repo/backend/src/state_machine.rs](repo/backend/src/state_machine.rs):
    - `TECH` drives Scheduled → EnRoute → OnSite → InProgress ↔ WaitingOnParts → Completed.
    - `SUPER` / `ADMIN` may move any live state to `Canceled`.
    - `InProgress ↔ WaitingOnParts` is the only bidirectional pair; once a work order
      enters a terminal state the row refuses further transitions.
  - `assigned_tech_id`, `branch_id`, `recipe_id`.
  - `sla_deadline` drives the 75/90/100% alert thresholds.
  - `etag` (SHA-256 hex), `version_count` (monotonic).
  - `on_call: bool` — automatic routing flag (PRD §7). The backend sets this to
    `true` on create and on transition when `priority = 'HIGH'` and the SLA
    deadline is within `ON_CALL_HIGH_PRIORITY_HOURS` (default **4**). Terminal
    states always clear it. Supervisors read `/api/work-orders/on-call-queue`
    which returns rows with `on_call = true`.
- `WorkOrderTransition` — append-only; the row is made immutable by a DB trigger.
  - `from_state`, `to_state`, `triggered_by` (user id), `created_at`,
    `required_fields` (JSONB), optional `notes`.

### 1.4 Recipes and steps

- `RecipeRecord`: template of ordered steps, reusable across work orders.
- `RecipeStepRecord`: ordered step with optional `step_timers` for timing.
- `StepTimerRecord`: duration, `alert_type` (e.g. visual/audible).
- `TipCardRecord`: contextual guidance attached to a step.

### 1.5 Step progress and versioning

- `JobStepProgressRecord`
  - `status`: `'Pending' | 'InProgress' | 'Paused' | 'Completed'`
  - `timer_state_snapshot` (JSONB) — paused timer state for resume.
  - `version` — monotonic; per-row history stored in `job_step_progress_versions`.
- Retention: at most `MAX_VERSIONS_PER_PROGRESS` (default **30**) historical versions per progress row.

### 1.6 Learning

- `KnowledgePointRecord` — quiz prompt, choices, correct answer.
- `LearningRecord` — per-user attempt with score and timestamps.

### 1.7 Location

- `LocationTrailRecord` — GPS ping (lat, lng, accuracy, at), optionally coarsened when privacy mode is on.
- `CheckInRecord` — arrival/departure markers tied to a work order.

### 1.8 Notifications

- `NotificationRecord`
  - `template_type`: enum
    `'SIGNUP_SUCCESS' | 'SCHEDULE_CHANGE' | 'CANCELLATION' | 'REVIEW_RESULT'`
    (source of truth: [repo/backend/src/enums.rs](repo/backend/src/enums.rs)).
    SLA warnings ride on `SCHEDULE_CHANGE` with an `sla_alert` payload key.
  - `retry_count`, `delivered_at?`, `is_unsubscribed`.

## 2) HTTP API surface

All routes are prefixed with `/api` and registered in [repo/backend/src/lib.rs](repo/backend/src/lib.rs). Endpoints return JSON envelopes and standard HTTP status codes; validation errors are 4xx with a structured body from `errors.rs`.

| Group             | Representative endpoints                                                            | Role scope          |
|-------------------|-------------------------------------------------------------------------------------|---------------------|
| `/auth`           | `POST /login`, `POST /logout`, `POST /change-password`                              | Public / self       |
| `/me`             | `GET /`, `PUT /privacy`, `PUT /home-address`                                        | Self                |
| `/work-orders`    | CRUD, `PUT /{id}/state`, `GET /{id}/timeline`, `GET /on-call-queue`                 | TECH (own) / SUPER / ADMIN |
| `/recipes/*`      | `recipes`, `steps`, `tip-cards` (read + manage)                                     | Read: all; Manage: SUPER+ |
| `/learning/*`     | `knowledge`, `records` (submit + list)                                              | Self; ADMIN sees all |
| `/analytics`      | `GET /learning` with date filter + CSV export                                       | SUPER (branch) / ADMIN (all) |
| `/notifications`  | `GET /`, `POST /{id}/read`, `POST /unsubscribe`                                     | Self                |
| `/sync/*`         | `GET /changes`, `POST /step-progress`, `GET /conflicts`, `POST /work-orders/{id}/delete` | Self                |
| `/admin/*`        | `users`, `branches`, `POST /retention/prune`, `POST /sync/trigger`                  | ADMIN               |
| `/health`         | `GET /health`, `GET /api/health`                                                    | Public              |

Common conventions:

- **Pagination**: list endpoints accept `page` and `per_page`; clamped by [pagination.rs](repo/backend/src/pagination.rs) (default **20**, max **200**). Responses use `{ items, page, per_page, total }`.
- **ETag / If-Match**: work orders and step-progress mutating endpoints require `If-Match: <etag>`; a missing or stale value returns `412 Precondition Failed`. Enforced by
  [repo/backend/src/work_orders/routes.rs](repo/backend/src/work_orders/routes.rs) and
  [repo/backend/src/work_orders/progress.rs](repo/backend/src/work_orders/progress.rs).
  Step-progress upsert only requires the header when a prior row exists (first-time inserts have no ETag to match).
- **Dates**: ISO-8601 UTC on the wire; analytics input/export uses `MM/DD/YYYY`.

## 3) Auth API

Primary module: [repo/backend/src/auth](repo/backend/src/auth).

### 3.1 Login — `POST /api/auth/login`

Request:

```json
{ "username": "admin", "password": "<plaintext>" }
```

Response:

```json
{
  "token": "<JWT>",
  "user": { "id": "...", "username": "...", "role": "ADMIN", "branch_id": null },
  "password_reset_required": true
}
```

Behavior:

- Password verified against Argon2 `password_hash`.
- JWT signed with `JWT_SECRET`, expiry `JWT_EXPIRY_HOURS` (default **24**).
- When `password_reset_required=true`, the client must route the user to change-password before any privileged action.

### 3.2 Change password — `POST /api/auth/change-password`

- Bearer-token protected (the token issued at login is accepted even when `password_reset_required=true`).
- Enforces min length **12** and `new != current`.
- Clears `password_reset_required`.

### 3.3 Logout — `POST /api/auth/logout`

- Client-driven: discards the token locally. The server is stateless with respect to revocation in this version.

## 4) Authorization API

Primary module: [repo/backend/src/middleware](repo/backend/src/middleware).

Contract:

- **Authentication** is enforced by the `JwtAuth` middleware wrapped around every non-auth scope.
- **Authorization** is enforced per-endpoint by `require_role` and by object-level scoping in queries (e.g., a TECH's work-order list is filtered by `assigned_tech_id = caller.id`; a SUPER's is filtered by `branch_id`).
- Frontend `can()` helpers are a UI convenience layered on top of server enforcement, not a substitute for it.

## 5) Work order + recipe API (editor behaviors)

Primary modules: [repo/backend/src/work_orders](repo/backend/src/work_orders), [repo/backend/src/recipes](repo/backend/src/recipes), [repo/backend/src/state_machine.rs](repo/backend/src/state_machine.rs).

Key contracts:

- Transitions are validated by the state machine; disallowed transitions return 400 with `required_fields` indicating what is missing.
- Each successful transition appends a row to `work_order_transitions` (immutable via DB trigger).
- Automatic on-call routing runs on every create + transition. When the rule
  flips the `on_call` flag, a dedicated `work_order.on_call.routed` row is
  written to `processing_log` so the routing event is individually auditable.
- Step progress mutations bump `version` and may produce a new entry in `job_step_progress_versions`, subject to the retention cap.
- Recipe step timers are server-owned; the client renders from the server-stored `timer_state_snapshot` so pause/resume survives disconnects.

## 6) Sync contract (offline-first)

Primary module: [repo/backend/src/sync](repo/backend/src/sync).

### 6.1 Pull changes — `GET /api/sync/changes?since=<cursor>`

Response:

```json
{
  "cursor": "<next-cursor>",
  "work_orders": [ /* changed work orders */ ],
  "step_progress": [ /* changed progress rows */ ],
  "notifications": [ /* new notifications */ ]
}
```

### 6.2 Push progress — `POST /api/sync/step-progress`

Request: batch of `JobStepProgressRecord` with `If-Match`-style ETags per row.

Response: per-row outcome: `applied`, `conflict` (stored for later), or `rejected` (validation).

### 6.3 Conflicts — `GET /api/sync/conflicts`

Returns the current conflict queue (server-side record of failed merges) so an operator or the user can resolve them explicitly. There is no silent "last write wins".

## 7) Notification retry contract

Primary module: [repo/backend/src/notifications](repo/backend/src/notifications).

- A background worker wakes on `NOTIFICATION_RETRY_BASE_SECONDS` and attempts undelivered notifications with exponential backoff, up to `NOTIFICATION_RETRY_MAX_ATTEMPTS` (default **5**).
- `is_unsubscribed=true` short-circuits further delivery attempts for that user/template.

## 8) Health and diagnostics

- `GET /health` and `GET /api/health` return a liveness payload; used by the Docker healthcheck.
- Admin-only endpoints under `/api/admin/*` allow triggering retention pruning and sync on demand for operational confirmation.

## 9) Non-goals (explicitly out of scope)

- Server-side JWT revocation / refresh token rotation (clients discard tokens on logout; rotation handled by short expiry).
- Multi-tenant isolation beyond per-branch scoping (FieldOps runs per deployment).
- Push channels beyond the in-app `notifications` table (no email/SMS/push in this version).
- Online third-party geocoding — address normalization runs entirely offline against the embedded ZIP+4 index.
