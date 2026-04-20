# FieldOps Kitchen & Training Console

**Project type:** full-stack web application.
**Stack:** Rust + actix-web backend (REST API, JSON over HTTP), Rust + Yew
frontend compiled to WebAssembly and served by nginx, PostgreSQL for
persistence. Docker Compose orchestrates the whole stack — no host toolchain
required beyond Docker.

One-click startup. No manual file creation, no `.env` copying.

## Start

```bash
docker compose up --build
```

First build is ~10 min (Rust -> WASM for the Yew frontend); subsequent builds hit the dependency layer cache.

## Access

Once `docker compose up` reports all three services healthy, the app is
reachable at:

| Surface                  | URL                                   | Notes                                      |
|--------------------------|---------------------------------------|--------------------------------------------|
| Web UI (Yew/WASM)        | http://localhost:8081                 | Open in a modern browser (Chrome/Firefox). |
| Backend API (direct)     | http://localhost:8080                 | Same backend the UI talks to via nginx.    |
| Backend health           | http://localhost:8080/health          | Returns `{"status":"ok"}` when ready.      |
| API proxy via frontend   | http://localhost:8081/api/health      | Proves nginx -> backend wiring.            |
| Postgres (for inspection)| `postgres://fieldops:fieldops_pw@localhost:5432/fieldops` | Credentials live in `docker-compose.yml`.  |

## Verify

The commands below prove each surface is up without touching a browser. Run
them from any shell after `docker compose up`:

```bash
# 1. Frontend serves the static bundle:
curl -sI http://localhost:8081/ | head -n1
# -> HTTP/1.1 200 OK

# 2. Backend health endpoint is reachable directly:
curl -s http://localhost:8080/health
# -> {"status":"ok"}

# 3. Nginx proxies /api/* from the frontend to the backend:
curl -s http://localhost:8081/api/health
# -> {"status":"ok"}

# 4. End-to-end login round-trip (returns a JWT):
curl -s -X POST http://localhost:8081/api/auth/login \
     -H 'Content-Type: application/json' \
     -d '{"username":"admin","password":"admin123"}'
# -> {"token":"eyJ...","user":{...},"password_reset_required":true}
```

The full acceptance suite runs the same assertions plus RBAC, workflow, and
failure-path journeys inside Docker via `./run_tests.sh`.

## Test

```bash
./run_tests.sh
```

Runs three Dockerized stages and a summary: backend unit + API tests,
frontend wasm unit tests (headless Chromium), and end-to-end smoke against
the live stack. Add `COVERAGE=1 ./run_tests.sh` to emit an HTML/XML line
coverage report to `.tmp/coverage/` via tarpaulin.

## Demo credentials — all three roles

> **DEVELOPMENT DEFAULTS (WARNING).** The seeded credentials below are
> intentionally insecure placeholders. The backend accepts them only because
> `DEV_MODE=true` is set in `docker-compose.yml`. They MUST NOT ship to
> production -- startup with `DEV_MODE=false` will hard-fail if any of
> `JWT_SECRET`, `AES_256_KEY_HEX`, or `DEFAULT_ADMIN_PASSWORD` still holds
> a known placeholder value.

The PRD defines three roles; the backend enums match: `TECH`, `SUPER`, `ADMIN`.

### 1. Seeded ADMIN (automatic)

Created on first boot from `docker-compose.yml` env.

| Field    | Value     |
|----------|-----------|
| Username | `admin`   |
| Password | `admin123`|
| Role     | `ADMIN`   |

The account is seeded with `password_reset_required = true` (because
`REQUIRE_ADMIN_PASSWORD_CHANGE=true`). On first login the server returns
`password_reset_required: true`; the operator must call
`POST /api/auth/change-password` (bearer-token protected) before performing
any privileged action. Change the seed via `DEFAULT_ADMIN_USERNAME` /
`DEFAULT_ADMIN_PASSWORD` in `docker-compose.yml`.

### 2. Provision SUPER and TECH (one-time, via the admin API)

`SUPER` and `TECH` accounts are not seeded — the PRD requires them to be
created by an admin so the audit log attributes the action. After the admin
rotation in step 1, run these commands with the admin's new bearer token:

```bash
# Rotate the admin password and capture the token for subsequent calls.
ADMIN_PASS=admin123
NEW_ADMIN_PASS=a-brand-new-long-enough-password   # >=12 chars

TOKEN=$(curl -s -X POST http://localhost:8081/api/auth/login \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"admin\",\"password\":\"${ADMIN_PASS}\"}" \
  | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')

curl -s -X POST http://localhost:8081/api/auth/change-password \
  -H "Authorization: Bearer ${TOKEN}" \
  -H 'Content-Type: application/json' \
  -d "{\"current_password\":\"${ADMIN_PASS}\",\"new_password\":\"${NEW_ADMIN_PASS}\"}"

# Create a SUPER (supervisor) — needs a branch_id. Create a branch first
# if one doesn't exist:
BRANCH_ID=$(curl -s -X POST http://localhost:8081/api/admin/branches \
  -H "Authorization: Bearer ${TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Main Branch","service_radius_miles":30}' \
  | sed -n 's/.*"id":"\([0-9a-f-]\{36\}\)".*/\1/p')

curl -s -X POST http://localhost:8081/api/admin/users \
  -H "Authorization: Bearer ${TOKEN}" \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"super_demo\",\"password\":\"super-demo-pw-long\",\"role\":\"SUPER\",\"branch_id\":\"${BRANCH_ID}\",\"full_name\":\"Demo Supervisor\"}"

# Create a TECH (technician) under the same branch:
curl -s -X POST http://localhost:8081/api/admin/users \
  -H "Authorization: Bearer ${TOKEN}" \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"tech_demo\",\"password\":\"tech-demo-pw-long\",\"role\":\"TECH\",\"branch_id\":\"${BRANCH_ID}\",\"full_name\":\"Demo Technician\"}"
```

Resulting demo accounts:

| Username     | Password              | Role   | Scope (for reads)                              |
|--------------|-----------------------|--------|------------------------------------------------|
| `admin`      | `${NEW_ADMIN_PASS}`   | ADMIN  | All branches, all users, all analytics.        |
| `super_demo` | `super-demo-pw-long`  | SUPER  | Own branch's work orders, team, and analytics. |
| `tech_demo`  | `tech-demo-pw-long`   | TECH   | Own work orders, own learning records.         |

Note: the backend enforces `password.len() >= 12` on user creation; the
sample passwords above satisfy that floor. Each created user lands with
`password_reset_required = false`, so they can log in and work immediately.

## Deploying to production

Before running in any shared environment:

1. **Turn off dev mode**: set `DEV_MODE=false` (or remove the env var). This
   enables the placeholder-rejection guard. The following values are the
   known placeholders and will be rejected:
   - `JWT_SECRET = "dev-jwt-secret-change-in-prod-0123456789abcdef"`
   - `AES_256_KEY_HEX = "0123456789abcdef..."` (the 32-byte repeating value
     shipped in `docker-compose.yml`)
   - `DEFAULT_ADMIN_PASSWORD` in `{ "admin", "admin123", "password", "123456" }`
2. **Generate secrets**:
   ```bash
   # 32-byte (64 hex char) AES key
   openssl rand -hex 32
   # 48+ byte JWT signing secret
   openssl rand -base64 48
   ```
3. **Inject via environment**, *not* checked into git. Use a secret manager
   (Vault, AWS Secrets Manager, Doppler, etc.) or the orchestrator's
   native secrets mount.
4. **Keep `REQUIRE_ADMIN_PASSWORD_CHANGE=true`** so the seeded admin must
   rotate their password on first login.
5. **Rotate `AES_256_KEY_HEX` carefully** -- previously encrypted values
   (`users.home_address_enc`) are unrecoverable after a rotation unless you
   write a migration that re-encrypts under the new key.

## Offline ZIP+4 normalization

Address geocoding runs entirely offline against a bundled index at
`backend/data/zip4_index.csv`. The sample file covers representative US
cities; swap in a real USPS-derived dataset before go-live. The file is
embedded at compile time via `include_str!`, so a rebuild is required
after updating it.
