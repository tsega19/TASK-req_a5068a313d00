# FieldOps Kitchen & Training Console — API Specifications

REST/JSON over HTTP, fronted by nginx at `http://localhost:8081/api/*`
and reachable directly at `http://localhost:8080/api/*` for service-to-
service callers. Single source of truth: `repo/backend/src/lib.rs::configure`
plus the route handlers under `repo/backend/src/*/routes.rs`.

This document is a *contract* between the Yew/WASM frontend and the
Actix-web backend. When this document and the code disagree, the code
wins — please file the discrepancy as a doc bug.

---

## 1. Conventions

### 1.1 Transport & encoding

- Transport: `http://` over the Compose network; `https://` is enabled
  by `ENABLE_TLS=true` + `TLS_CERT_PATH` / `TLS_KEY_PATH`.
- Request bodies: `application/json` (UTF-8). Empty body for `GET`/`DELETE`.
- Response bodies: `application/json` (UTF-8) for everything except CSV
  exports, which return `text/csv`.

### 1.2 Authentication header (signed request header)

Every non-auth endpoint requires:

```
Authorization: Bearer <JWT>
```

- The JWT is HS256-signed with `JWT_SECRET`; claims include `sub`
  (user_id), `username`, `role`, `branch_id`, `iss`, `aud`, `iat`,
  `exp`. `iss` and `aud` are validated on every request — a token
  minted for one deployment is rejected by another.
- Default expiry: `JWT_EXPIRY_HOURS = 24`.
- If `password_reset_required=true` for the caller, every protected
  endpoint **except** `POST /api/auth/change-password` and
  `GET /api/me` rejects with **403 forbidden**
  (`code = "password_reset_required"`).

### 1.3 ETag / version concurrency

Work-order rows and step-progress rows both expose:

- `etag` — SHA-256 of the row's sync-relevant fields.
- `version` (or `version_count`) — monotonic counter.

The values are **returned in the JSON body**, not in HTTP `ETag` /
`If-Match` headers. Sync push endpoints accept the row's prior etag
inline in the request payload; mismatched etags become **conflict**
outcomes routed to `/api/sync/conflicts` rather than HTTP `412`.

### 1.4 Date encoding

- ISO-8601 UTC (`2026-04-28T13:38:00Z`) on the wire for everything that
  is not user-visible analytics input.
- The analytics endpoints accept and emit `MM/DD/YYYY` to match PRD
  §9.

### 1.5 Pagination

List endpoints accept query params:

| Param      | Type     | Default | Bounds       | Notes |
|------------|----------|---------|--------------|-------|
| `page`     | integer  | `1`     | `>= 1`       | 1-indexed |
| `per_page` | integer  | `20`    | `[1, 200]`   | clamped server-side in `pagination.rs` |

Response envelope for paginated endpoints:

```json
{
  "data": [ /* T */ ],
  "page": 1,
  "per_page": 20,
  "total": 137
}
```

Non-paginated list endpoints (e.g. `/api/sync/conflicts`,
`/api/admin/record-versions`) return:

```json
{ "data": [ /* T */ ], "total": 7 }
```

### 1.6 Filtering / sorting

| Endpoint                         | Filters                                                  | Sort |
|----------------------------------|----------------------------------------------------------|------|
| `GET /api/work-orders`           | `state`, `priority`, `branch_id`, `assigned_tech_id`     | SLA-deadline ascending (most urgent first) |
| `GET /api/work-orders/on-call-queue` | (no filters; SUPER+ scoped automatically)            | `priority` desc, then `sla_deadline` asc |
| `GET /api/recipes`               | (none)                                                   | `name` asc |
| `GET /api/learning-records`      | `user_id`, `recipe_id`, `start`, `end`                   | `created_at` desc |
| `GET /api/notifications`         | `unread=true` (boolean)                                  | `created_at` desc |
| `GET /api/analytics/learning`    | `start`, `end` (`MM/DD/YYYY`), `branch`, `role`          | grouped by knowledge point |
| `GET /api/sync/changes`          | `since` (cursor)                                         | `created_at` asc |
| `GET /api/admin/processing-log`  | `actor`, `entity_table`, `start`, `end`                  | `created_at` desc |

### 1.7 Error response shape

Every error response carries:

```json
{ "error": "human-readable message", "code": "machine_code" }
```

Status codes used:

| Status | `code`                   | When |
|-------:|--------------------------|------|
| 400    | `bad_request`            | malformed body, missing required field, invalid date |
| 401    | `unauthorized`           | missing / invalid JWT, expired token, bad credentials |
| 403    | `forbidden`              | role mismatch, scope mismatch, `password_reset_required` |
| 404    | `not_found`              | resource missing **or** out-of-scope (anti-enumeration) |
| 409    | `conflict`               | unique constraint violation (e.g. duplicate username) |
| 412    | (not used)               | concurrency is handled inline via the sync push outcome |
| 422    | `bad_request`            | state-machine transition rejected; `required_fields` listed in body |
| 429    | (not exposed yet)        | rate-limit hits surface via the notification queue, not HTTP |
| 500    | `internal_error`         | unexpected; public message is generic, server logs detail |

---

## 2. Authentication API

Module: `repo/backend/src/auth/routes.rs`. Scope: `/api/auth`. Public.

### 2.1 `POST /api/auth/login`

Request body:

```json
{ "username": "admin", "password": "admin123" }
```

Response **200**:

```json
{
  "token": "eyJhbGciOi…",
  "user": {
    "id": "9f8e7d6c-…",
    "username": "admin",
    "role": "ADMIN",
    "branch_id": null,
    "full_name": "Default Admin",
    "privacy_mode": false
  },
  "password_reset_required": true
}
```

Errors: `401 unauthorized` on bad credentials.

### 2.2 `POST /api/auth/change-password`

Auth: bearer token (accepted even when `password_reset_required=true`).

Request body:

```json
{ "current_password": "admin123", "new_password": "a-brand-new-long-enough-password" }
```

Response **200**: `{ "ok": true }`. Clears `password_reset_required`.

Errors:

- `400 bad_request` — `new_password.len() < 12` or `new == current`.
- `401 unauthorized` — `current_password` mismatch.

### 2.3 `POST /api/auth/logout`

Auth: bearer token. Stateless — the client discards the token. The
server records the action in `processing_log`.

Response **200**: `{ "ok": true }`.

---

## 3. Self-service (`/api/me`)

Module: `repo/backend/src/me/routes.rs`. Auth: any authenticated user.

### 3.1 `GET /api/me`

Response **200**:

```json
{
  "id": "…",
  "username": "tech_demo",
  "role": "TECH",
  "branch_id": "…",
  "full_name": "Demo Technician",
  "privacy_mode": false,
  "password_reset_required": false
}
```

### 3.2 `PUT /api/me/privacy`

Request body: `{ "privacy_mode": true }`

Response **200**: `{ "privacy_mode": true }`

Effect: subsequent location-trail and check-in writes coarsen GPS
precision; non-admin readers cannot see this user's trail.

### 3.3 `PUT /api/me/home-address` and `GET /api/me/home-address`

`PUT` body: `{ "home_address": "123 Main St, …" }`. Encrypted at rest
with AES-256-GCM. The audit log records *the action*, never the
plaintext.

`GET` returns `{ "stored": true, "home_address": "123 Main St, …" }`
(decrypted only for the owner) or `{ "stored": false, "home_address": null }`.

---

## 4. Work orders (`/api/work-orders`)

Module: `repo/backend/src/work_orders/routes.rs`. Auth required.

### 4.1 `GET /api/work-orders`

Query: `page`, `per_page`, `state`, `priority`, `branch_id`,
`assigned_tech_id`. RBAC scope is enforced regardless of query — a TECH
asking for `assigned_tech_id` other than their own gets back an empty
`data`.

Response **200** (paginated):

```json
{
  "data": [
    {
      "id": "…",
      "title": "Walk-in cooler stabilization",
      "priority": "HIGH",
      "state": "Scheduled",
      "assigned_tech_id": "…",
      "branch_id": "…",
      "recipe_id": "…",
      "address": "200 Market St, San Francisco CA 94103",
      "lat": 37.79, "lng": -122.4,
      "sla_deadline": "2026-04-28T20:00:00Z",
      "etag": "8d2e…",
      "version_count": 3,
      "created_at": "…",
      "updated_at": "…"
    }
  ],
  "page": 1, "per_page": 20, "total": 1
}
```

### 4.2 `POST /api/work-orders`  *(SUPER, ADMIN)*

Request body:

```json
{
  "title": "…",
  "priority": "HIGH",
  "address": "200 Market St, San Francisco CA 94103",
  "branch_id": "…",
  "assigned_tech_id": "…",
  "recipe_id": "…",
  "sla_deadline": "2026-04-28T20:00:00Z"
}
```

Response **201**: full `WorkOrder` (same shape as §4.1 element).

Auto-routing rule: if `priority` is `HIGH`/`CRITICAL` and
`sla_deadline - now <= ON_CALL_HIGH_PRIORITY_HOURS`, the request is
re-routed to the best on-call TECH; an `auto_dispatch` audit row is
written.

### 4.3 `GET /api/work-orders/{id}`

Response **200**: `WorkOrder`. **404** for any caller outside the
visibility scope (TECH not assigned, SUPER outside branch).

### 4.4 `PUT /api/work-orders/{id}/state`

Request body:

```json
{
  "to": "InProgress",
  "notes": "optional, required for WaitingOnParts and Canceled",
  "lat": 37.79, "lng": -122.4
}
```

Required-field rules (enforced server-side in `state_machine.rs`):

| From → To              | Required |
|------------------------|----------|
| `Scheduled → EnRoute`  | `lat`, `lng` |
| `EnRoute → OnSite`     | An `ARRIVAL` check-in inside `service_radius_miles` of the branch |
| `InProgress → WaitingOnParts` | non-empty `notes` |
| `InProgress → Completed` | every step's progress = `Completed`, plus a `DEPARTURE` check-in |
| `* → Canceled`         | non-empty `notes` (SUPER/ADMIN only) |

Response **200**: updated `WorkOrder`.

Errors:

- `400 bad_request` — required field missing.
- `403 forbidden` — caller's role cannot perform this edge.
- `422 bad_request` — transition not in the allowed graph.

### 4.5 `GET /api/work-orders/{id}/timeline`

Response **200**:

```json
{
  "data": [
    {
      "from_state": "Scheduled",
      "to_state": "EnRoute",
      "triggered_by": "…",
      "at": "…",
      "required_fields": { "lat": 37.79, "lng": -122.4 },
      "notes": null
    }
  ]
}
```

### 4.6 `DELETE /api/work-orders/{id}`  *(SUPER, ADMIN)*

Soft delete. Sets `deleted_at`. Hard removal happens via the retention
worker after `SOFT_DELETE_RETENTION_DAYS` (default 90).

### 4.7 `GET /api/work-orders/on-call-queue`  *(SUPER, ADMIN)*

Returns the live on-call view: HIGH/CRITICAL work orders within
`ON_CALL_HIGH_PRIORITY_HOURS` of their SLA, ordered by priority then
deadline.

### 4.8 Step progress

#### `GET /api/work-orders/{id}/progress`

Response **200**: `{ "data": [JobStepProgress, …] }`.

#### `PUT /api/work-orders/{id}/steps/{step_id}/progress`

Request body:

```json
{
  "status": "InProgress",
  "notes": "optional",
  "timer_state_snapshot": { "timer_id_a": { "remaining_ms": 720000, "started_at": "…" } },
  "etag": "<current row etag>",
  "version": 4
}
```

Response **200**: updated `JobStepProgress` with new `etag` + `version`.

### 4.9 Location trail & check-ins

| Method | Path | Auth |
|--------|------|------|
| `POST` | `/api/work-orders/{id}/location-trail` | TECH (own), SUPER, ADMIN |
| `GET`  | `/api/work-orders/{id}/location-trail` | TECH (own), SUPER (branch), ADMIN; privacy mode hides from non-admin |
| `POST` | `/api/work-orders/{id}/check-in`       | TECH (own) |

Trail post body: `{ "lat": 37.79, "lng": -122.4, "accuracy_m": 8.0, "at": "…" }`.

Check-in post body: `{ "type": "ARRIVAL", "lat": 37.79, "lng": -122.4, "at": "…" }`.

---

## 5. Recipes (`/api/recipes`, `/api/steps`, `/api/tip-cards`)

Module: `repo/backend/src/recipes/routes.rs`.

| Method | Path | Auth |
|--------|------|------|
| `GET`  | `/api/recipes` | any |
| `POST` | `/api/recipes` | SUPER, ADMIN |
| `PUT`  | `/api/recipes/{id}` | SUPER, ADMIN |
| `GET`  | `/api/recipes/{id}/steps` | any |
| `GET`  | `/api/recipes/{id}/timers` | any |
| `GET`  | `/api/recipes/{id}/tip-cards` | any |

`Recipe`: `{ id, name, description, created_by, created_at, updated_at }`.

`RecipeStep`: `{ id, recipe_id, position, title, body, duration_seconds }`.

`StepTimer`: `{ id, step_id, label, duration_seconds, alert_type ('AUDIBLE'|'VISUAL'|'BOTH') }`.

`TipCard`: `{ id, step_id, title, body, authored_by, created_at }`.

---

## 6. Learning (`/api/knowledge-points`, `/api/learning-records`)

Module: `repo/backend/src/learning/routes.rs`.

| Method | Path | Auth |
|--------|------|------|
| `GET`  | `/api/knowledge-points` | any |
| `GET`  | `/api/knowledge-points/by-step/{step_id}` | any |
| `POST` | `/api/knowledge-points` | SUPER, ADMIN |
| `PUT`  | `/api/knowledge-points/{id}` | SUPER, ADMIN |
| `DELETE` | `/api/knowledge-points/{id}` | SUPER, ADMIN |
| `POST` | `/api/learning-records` | any (TECH submits own) |
| `GET`  | `/api/learning-records` | TECH (own) / SUPER (branch) / ADMIN (all) |
| `GET`  | `/api/learning-records/{id}` | scope-checked |

`KnowledgePoint`: `{ id, recipe_id, step_id?, prompt, choices: string[], correct_index }`.

`LearningRecord` (POST body): `{ knowledge_point_id, work_order_id?, score, time_spent_seconds }`.

Response shape mirrors the request plus `id`, `user_id`, `created_at`.

---

## 7. Analytics (`/api/analytics`)

Module: `repo/backend/src/analytics/routes.rs`. Auth: SUPER (branch),
ADMIN (all). TECH receives 403 (their own learning history is
available via `/api/learning-records`).

### 7.1 `GET /api/analytics/learning`

Query: `start`, `end` (`MM/DD/YYYY`), `branch` (UUID, ignored if
caller is SUPER outside that branch — server overrides), `role`.

Response **200**:

```json
{
  "data": [
    {
      "knowledge_point_id": "…",
      "prompt": "EPA recovery procedure for R-410A",
      "completion_rate": 0.87,
      "average_score": 0.81,
      "time_spent_seconds": 5400,
      "review_count": 12
    }
  ],
  "filters": { "start": "04/01/2026", "end": "04/28/2026", "branch": "…", "role": null }
}
```

### 7.2 `GET /api/analytics/learning/export-csv`

Same query params. Returns `text/csv` with a watermark header line
`# exported_by=<username> at <ISO-8601>`.

### 7.3 Trend endpoints

| Path | Notes |
|------|-------|
| `GET /api/analytics/trends/knowledge-points` | per-KP time series |
| `GET /api/analytics/trends/units` | per learning unit |
| `GET /api/analytics/trends/workflows` | per recipe / workflow |

All scope-respecting; same filter params as §7.1.

---

## 8. Notifications (`/api/notifications`)

Module: `repo/backend/src/notifications/routes.rs`. Auth: any. All
routes object-scoped to the caller.

| Method | Path | Body |
|--------|------|------|
| `GET`  | `/api/notifications` | — (paginated; `unread=true` filter) |
| `PUT`  | `/api/notifications/{id}/read` | — |
| `PUT`  | `/api/notifications/unsubscribe` | `{ "template": "SCHEDULE_CHANGE" }` |

`Notification`: `{ id, user_id, template, payload, retry_count, delivered_at?, read_at?, is_unsubscribed, created_at }`.

Templates: `SIGNUP_SUCCESS | SCHEDULE_CHANGE | CANCELLATION | REVIEW_RESULT`. Rate
limit: `MAX_NOTIFICATIONS_PER_HOUR` per user (default 20). Retry
ladder: up to `NOTIFICATION_RETRY_MAX_ATTEMPTS` (default 5) with
exponential backoff seeded at `NOTIFICATION_RETRY_BASE_SECONDS`.

---

## 9. Sync (`/api/sync`)

Module: `repo/backend/src/sync/routes.rs`. Auth required.

### 9.1 `GET /api/sync/changes?since=<cursor>`

Response **200**:

```json
{
  "cursor": "2026-04-28T13:38:00.000Z",
  "work_orders": [ { "id": "…", "etag": "…", "operation": "UPDATE" } ],
  "step_progress": [ { "id": "…", "etag": "…", "operation": "INSERT" } ],
  "notifications": [ { "id": "…", "operation": "INSERT" } ]
}
```

Server scopes the response to whatever the caller can already see on
the live read endpoints.

### 9.2 `POST /api/sync/step-progress`

Request body:

```json
{
  "id": "<job_step_progress.id>",
  "work_order_id": "…",
  "step_id": "…",
  "status": "InProgress",
  "notes": "…",
  "timer_state_snapshot": {},
  "version": 4,
  "etag": "<the etag the client last saw>"
}
```

Response **200**:

```json
{ "outcome": "applied", "conflict": false }
```

`outcome ∈ { "applied", "rejected_completed", "rejected_older", "conflict" }`.
Conflicts land in `sync_log` with `conflict_flagged=true` and surface
via §9.3.

### 9.3 `GET /api/sync/conflicts`  *(SUPER, ADMIN)*

```json
{
  "data": [
    {
      "id": "<sync_log.id>",
      "entity_table": "job_step_progress",
      "entity_id": "…",
      "new_etag": "…",
      "synced_at": "…"
    }
  ],
  "total": 1
}
```

### 9.4 `POST /api/sync/conflicts/{id}/resolve`  *(SUPER, ADMIN)*

Request body: `{ "acknowledged": true }`. Marks the conflict resolved,
recording the actor in `sync_log.conflict_resolved_by`. Both the
business write and the audit row commit in one transaction.

### 9.5 `POST /api/sync/work-orders/{id}/delete`

Soft-delete via the sync channel for clients that buffered a delete
while offline. Same effect as `DELETE /api/work-orders/{id}`.

---

## 10. Admin (`/api/admin`)  *(ADMIN only)*

Module: `repo/backend/src/admin/routes.rs`.

| Method | Path | Body |
|--------|------|------|
| `GET`  | `/users` | — |
| `POST` | `/users` | `{ username, password, role, branch_id?, full_name }` |
| `PUT`  | `/users/{id}` | partial update + `password?` (forces `password_reset_required=true` if rotated) |
| `DELETE` | `/users/{id}` | — (soft delete) |
| `GET`  | `/branches` | — |
| `POST` | `/branches` | `{ name, address, lat?, lng?, service_radius_miles }` |
| `PUT`  | `/branches/{id}` | partial update |
| `POST` | `/sync/trigger` | — |
| `POST` | `/retention/prune` | — |
| `POST` | `/notifications/retry` | — |
| `POST` | `/sla/scan` | — |
| `POST` | `/dispatch/scan` | — |
| `GET`  | `/processing-log` | filterable: `actor`, `entity_table`, `start`, `end` |
| `GET`  | `/record-versions?entity_table=…&entity_id=…&limit=…` | per-row history |

Operational triggers return a JSON report describing what the worker
did (e.g. `{ "scanned": 12, "delivered": 8, "giveup": 1, "skipped_backoff": 3 }`).
Every trigger lands a row in `processing_log`.

Constraints:

- `branch_id` is **required** for `SUPER` and `TECH` (migration `0005`).
- New passwords must be ≥12 chars and differ from the current one
  (auth-side guard).

---

## 11. Location helpers (`/api/location`)

| Method | Path | Body |
|--------|------|------|
| `POST` | `/api/location/geocode` | `{ "address": "200 Market St, …" }` |

Response **200**: `{ "lat": 37.79, "lng": -122.4, "zip4": "94103-1234" }`. Lookup runs against the embedded
`zip4_index.csv`; out-of-index addresses return **404**.

---

## 12. Health

| Method | Path | Auth |
|--------|------|------|
| `GET`  | `/health`     | public |
| `GET`  | `/api/health` | public |

Response **200**: `{ "status": "ok" }`. Used by the Compose
healthcheck and by the verification curls in `repo/README.md`.

---

## 13. Non-goals

- Server-side JWT revocation / refresh-token rotation. Logout is
  client-side; rotation handled by short token expiry.
- Multi-tenant isolation beyond per-branch scoping. FieldOps is
  one deployment per organization.
- External notification channels (email/SMS/push). The PRD requires
  in-app only in offline mode.
- Online geocoding services. Address normalization is purely offline
  against the embedded ZIP+4 index.
