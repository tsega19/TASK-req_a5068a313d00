# FieldOps Kitchen & Training Console - Questions (4) with Assumption and Reason

This document captures four high-impact questions about scope and behavior. Each includes:

- **Answer**: the expected behavior in the product.
- **Assumption**: what we assume to be true about environment, users, or deployment.
- **Reason**: why this answer is correct and how we enforce it in code/config.

## 1) Is there a backend API?

**Answer:** Yes. FieldOps ships a Rust/Actix backend on port 8080 that serves a REST API under `/api/*`, backed by PostgreSQL. The Yew/WASM frontend is a thin client that calls the same API through an nginx reverse proxy.

**Assumption:** The deployment environment has network access between the browser, the nginx-served frontend, and the backend container, and a reachable Postgres instance. Clients may be intermittently offline, but the authoritative state lives on the server.

**Reason:** The backend owns enforcement of RBAC, state machines, retention, and encryption — concerns that cannot be delegated to a browser. Route groups are registered in [backend/src/lib.rs](repo/backend/src/lib.rs): `/api/auth`, `/api/me`, `/api/work-orders`, `/api/recipes/*`, `/api/learning/*`, `/api/sync/*`, `/api/notifications`, `/api/analytics`, `/api/admin/*`, plus `/health`. The internal shape of those endpoints is documented in [doc/api_aspec.md](doc/api_aspec.md).

## 2) How is "offline-first" handled?

**Answer:** The frontend can queue work while a technician is offline and reconcile with the backend via dedicated sync endpoints. Work orders and step progress carry an ETag (SHA-256) and a monotonic version; on push, the server detects conflicts, stores them, and exposes them through `/api/sync/conflicts` for later resolution.

**Assumption:** A single user edits a given work order from one device at a time in the common case; true concurrent edits are rare but must be captured rather than silently overwritten. The client device is a modern browser capable of running a WASM bundle and caching static assets via the shipped nginx layer.

**Reason:** Keeping state machine transitions and version caps on the server prevents drift when devices re-join. The ETag check lives in [backend/src/etag.rs](repo/backend/src/etag.rs); per-progress version retention is capped by `MAX_VERSIONS_PER_PROGRESS` (default `30`). The sync surface is exposed under `/api/sync/*` (changes pull, step-progress push, conflicts, work-order deletes) per [backend/src/sync](repo/backend/src/sync).

## 3) Are roles enforced?

**Answer:** Yes, role-based access is enforced on the server in two layers: a JWT authentication middleware that rejects anonymous requests, and per-route `require_role` guards that restrict sensitive endpoints to `ADMIN` or `SUPER`. In addition, object-level visibility scopes queries so a `TECH` only sees their own work orders and a `SUPER` only sees their branch.

**Assumption:** The three roles (`TECH`, `SUPER`, `ADMIN`) are sufficient for the operational model (technician, branch supervisor, administrator). Role changes are rare and happen through the admin panel. The JWT secret is rotated out of the shipped placeholder before any non-dev deployment.

**Reason:** Unlike a single-user local app, FieldOps handles scheduling and PII (encrypted home addresses), so UI gating alone is insufficient. The middleware lives in [backend/src/middleware](repo/backend/src/middleware); `DEV_MODE=false` additionally refuses to boot if `JWT_SECRET`, `AES_256_KEY_HEX`, or `DEFAULT_ADMIN_PASSWORD` still hold known placeholders (see [README](repo/README.md) "Deploying to production").

## 4) What are the key operational limits and constraints?

**Answer:** Pagination is capped (default 20, max 200 per page). Step-progress versions are capped at 30 per record. Soft-deleted rows are hard-deleted after 90 days by a retention worker. Notification delivery retries up to 5 times with exponential backoff. SLA alerts fire at 75%, 90%, and 100% of the work-order deadline. Passwords must be at least 12 characters and cannot match the current one.

**Assumption:** Operators may import or generate large data sets, but the system must stay responsive and bounded in storage. The default limits match a "small/medium branch" footprint; tuning per-deployment is done via environment variables, not code changes.

**Reason:** These numbers are enforced in service code and configuration, not merely documented. Pagination clamps live in [backend/src/pagination.rs](repo/backend/src/pagination.rs); retention uses `SOFT_DELETE_RETENTION_DAYS` in [backend/src/retention.rs](repo/backend/src/retention.rs); notification retry uses `NOTIFICATION_RETRY_MAX_ATTEMPTS` / `NOTIFICATION_RETRY_BASE_SECONDS`; SLA thresholds come from `SLA_ALERT_THRESHOLDS`. The `work_order_transitions` table is made immutable by a trigger so the audit trail cannot be rewritten.
