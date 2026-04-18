# FieldOps Kitchen & Training Console - Design

Rust/Actix backend + Yew/WASM frontend + PostgreSQL. Three containers deployed together via Docker Compose. Designed for field technicians with intermittent connectivity: authoritative state lives on the server, but the client can queue and reconcile through sync endpoints.

## Module layout

```
repo/
|-- backend/
|   |-- config/                     # Typed env-var loader + placeholder guard (DEV_MODE)
|   |-- migrations/                 # sqlx migrations; 0001_init.sql builds the schema
|   |-- data/zip4_index.csv         # Embedded offline ZIP+4 index (compile-time include_str!)
|   |-- logging/                    # Structured logger (JSON or pretty)
|   `-- src/
|       |-- lib.rs / main.rs        # App wiring: pool, workers, route groups, health
|       |-- auth/                   # Login, JWT issuance, password rotation, Argon2 hashing
|       |-- me/                     # Self-service profile, privacy mode, encrypted address
|       |-- work_orders/            # CRUD, state transitions, on-call queue, SLA tracking
|       |-- recipes/                # Recipes, steps, timers, tip cards
|       |-- learning/               # Knowledge points, quizzes, learning records
|       |-- analytics/              # Learning metrics export (CSV, date-filtered)
|       |-- notifications/          # Templates, retry worker, unsubscribe
|       |-- sync/                   # Pull changes, push progress, conflict store
|       |-- admin/                  # User/branch CRUD, retention prune, sync trigger
|       |-- location/               # GPS trails, check-ins, geocoding
|       |-- middleware/             # JwtAuth, require_role, request logging
|       |-- crypto.rs               # AES-256-GCM + Argon2 + SHA-256 ETag helpers
|       |-- db.rs                   # sqlx pool + common query helpers
|       |-- enums.rs                # Role, WorkOrderState, Priority, etc.
|       |-- errors.rs               # ApiError + HTTP mapping
|       |-- etag.rs                 # SHA-256 ETag compute + If-Match gate
|       |-- geo.rs                  # Privacy-mode rounding, distance, radius
|       |-- pagination.rs           # page/per_page clamping, Page<T> envelope
|       |-- retention.rs            # Soft-delete pruner
|       `-- state_machine.rs        # Work-order state transitions + required fields
`-- frontend/
    |-- index.html, Trunk.toml      # Yew bootstrap (built with Trunk)
    |-- nginx.conf                  # Static + proxy /api/* to backend
    `-- src/
        |-- main.rs / app.rs        # Root, theme, route dispatch
        |-- routes.rs               # Route enum (login, dashboard, work-order, admin, ...)
        |-- auth.rs                 # AuthCtx (reducer), localStorage persistence
        |-- api.rs                  # Fetch client with bearer token
        |-- pages/                  # One component per route
        |-- components/             # Shared: timer ring, state badge, SLA, nav, toast
        `-- types.rs                # Shared DTOs mirroring backend JSON shapes
```

## Config flow

1. [docker-compose.yml](repo/docker-compose.yml) injects environment variables into the backend container at start.
2. The backend's `config` module parses them into a typed struct with defaults; missing/invalid values fail fast with a clear error.
3. When `DEV_MODE=false`, a placeholder-rejection guard refuses to boot if `JWT_SECRET`, `AES_256_KEY_HEX`, or `DEFAULT_ADMIN_PASSWORD` still hold known placeholders (see [README](repo/README.md)).
4. Migrations run on boot when `RUN_MIGRATIONS_ON_BOOT=true`.
5. A default `ADMIN` user is seeded when `SEED_DEFAULT_ADMIN=true`; it is marked `password_reset_required=true` so the operator must rotate before any privileged action.

## Routing and guards

- All non-auth endpoints require a valid JWT (attached as `Authorization: Bearer <token>`), enforced by the `JwtAuth` middleware.
- Sensitive endpoints (admin panel, retention prune, sync trigger) add a per-route `require_role` guard.
- The frontend uses a single `AuthCtx` at the root; guarded pages redirect to `/login` when the token is missing or expired.

## Roles - enforced server-side (not just UI)

| Capability                                     | TECH | SUPER | ADMIN |
|------------------------------------------------|:----:|:-----:|:-----:|
| auth.login / me.read / me.privacy              |  X   |   X   |   X   |
| work_order.read (own)                          |  X   |   X   |   X   |
| work_order.read (branch)                       |      |   X   |   X   |
| work_order.read (all)                          |      |       |   X   |
| work_order.create / assign / transition        |      |   X   |   X   |
| recipe.read                                    |  X   |   X   |   X   |
| recipe.create / edit                           |      |   X   |   X   |
| learning.read / submit                         |  X   |   X   |   X   |
| analytics.query (branch scope)                 |      |   X   |   X   |
| analytics.query (all branches)                 |      |       |   X   |
| sync.pull / push (own)                         |  X   |   X   |   X   |
| admin.users / admin.branches / retention.prune |      |       |   X   |
| notifications.read / unsubscribe (own)         |  X   |   X   |   X   |

Frontend `can()` helpers hide affordances that would 403 anyway, but the server remains the authority.

## PostgreSQL schema (highlights)

Database provisioned by [backend/migrations/0001_init.sql](repo/backend/migrations/0001_init.sql). All timestamps are `TIMESTAMPTZ`.

| Table                        | Notes |
|------------------------------|-------|
| users                        | `role`, `branch_id`, Argon2 `password_hash`, AES-256-GCM `home_address_enc`, `privacy_mode`, `password_reset_required` |
| branches                     | `name`, address, `lat`/`lng`, `service_radius_miles` |
| work_orders                  | `priority`, `state` (7 states), `assigned_tech_id`, `sla_deadline`, `recipe_id`, `etag`, `version_count` |
| work_order_transitions       | Immutable audit of state changes (trigger blocks UPDATE/DELETE); `required_fields` (JSONB) |
| recipes / recipe_steps / step_timers | Training templates with pauseable steps |
| tip_cards                    | Contextual guidance per step |
| job_step_progress            | Per-tech progress; `timer_state_snapshot` (JSONB), `version` |
| job_step_progress_versions   | Version history; capped at `MAX_VERSIONS_PER_PROGRESS` |
| location_trails / check_ins  | GPS trails + arrival/departure markers |
| knowledge_points             | Quiz content |
| learning_records             | Per-user completion + score |
| notifications                | Template, `retry_count`, `delivered_at`, `is_unsubscribed` |

## Hard constraints

- Pagination: default `20`, min `1`, max `200` per page (clamped in [pagination.rs](repo/backend/src/pagination.rs)).
- Step-progress version retention: `MAX_VERSIONS_PER_PROGRESS` (default `30`).
- Soft-delete retention: `SOFT_DELETE_RETENTION_DAYS` (default `90`).
- Notification retry: `NOTIFICATION_RETRY_MAX_ATTEMPTS` (default `5`), `NOTIFICATION_RETRY_BASE_SECONDS` (default `1`, exponential backoff).
- SLA alerts: `SLA_ALERT_THRESHOLDS` (default `"0.75,0.90,1.00"`).
- Service radius: `DEFAULT_SERVICE_RADIUS_MILES` (default `30`).
- Minimum password length: 12; new password must differ from current.
- JWT expiry: `JWT_EXPIRY_HOURS` (default `24`).

## Background workers

Three async workers are spawned alongside the HTTP server:

| Worker              | Cadence                                 | Purpose |
|---------------------|-----------------------------------------|---------|
| sync ticker         | every `SYNC_INTERVAL_MINUTES` (def. 10) | Emits periodic sync cues; used by clients on refresh |
| notification retry  | `NOTIFICATION_RETRY_BASE_SECONDS` backoff | Re-attempts undelivered notifications up to the cap |
| retention pruner    | daily                                   | Hard-deletes rows whose `deleted_at` is older than the retention window |

## ETag / sync contract

- Work orders and step-progress rows carry an `etag` (SHA-256) and a monotonic `version`.
- Mutating requests should send `If-Match: <etag>`; a stale value returns HTTP 412 and the client is expected to resync.
- Offline clients push progress to `/api/sync/step-progress`; server-detected conflicts are persisted and surfaced at `/api/sync/conflicts` for later resolution.

## Logging

Structured logs via the `logging` crate set up in [backend/logging](repo/backend/logging). `LOG_FORMAT` controls JSON vs. pretty output; `LOG_LEVEL` controls verbosity. Sensitive fields (password, tokens, home address) are redacted before emit.

## Diagnostics

- `GET /health` and `GET /api/health` return a lightweight status payload; used by the Docker healthcheck.
- Admin endpoints expose retention triggers and sync triggers for operational confirmation.

## Backup and restore

This project relies on PostgreSQL's own dump/restore tooling (`pg_dump` / `pg_restore`) rather than a bespoke JSON bundle. The immutable `work_order_transitions` table must be preserved verbatim on restore; destructive wipes (schema drops) are an explicit operator action and are not exposed through the HTTP API.
