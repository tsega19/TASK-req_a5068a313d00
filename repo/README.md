# FieldOps Kitchen & Training Console

One-click startup. No manual file creation, no `.env` copying.

## Start

```bash
docker compose up --build
```

First build is ~10 min (Rust → WASM for the Yew frontend); subsequent builds hit the dependency layer cache.

## Ports

| Service   | Container | Host       |
|-----------|-----------|------------|
| Postgres  | 5432      | 5432       |
| Backend   | 8080      | 8080       |
| Frontend  | 80        | 8081       |

## Test

```bash
./run_tests.sh
```

## Default admin (development only)

> ⚠️  **DEVELOPMENT DEFAULTS.** The seeded credentials below are
> intentionally insecure placeholders. The backend accepts them only because
> `DEV_MODE=true` is set in `docker-compose.yml`. They MUST NOT ship to
> production — startup with `DEV_MODE=false` will hard-fail if any of
> `JWT_SECRET`, `AES_256_KEY_HEX`, or `DEFAULT_ADMIN_PASSWORD` still holds
> a known placeholder value.

A default `ADMIN` account is seeded on first boot:

- Username: `admin`
- Password: `admin123`

The account is created with `password_reset_required = true` (because
`REQUIRE_ADMIN_PASSWORD_CHANGE=true`). On first login the server returns
`password_reset_required: true`; the operator must call
`POST /api/auth/change-password` (bearer-token protected) before performing
any privileged action.

Change the seed via `DEFAULT_ADMIN_USERNAME` / `DEFAULT_ADMIN_PASSWORD` in
`docker-compose.yml`.

## Deploying to production

Before running in any shared environment:

1. **Turn off dev mode**: set `DEV_MODE=false` (or remove the env var). This
   enables the placeholder-rejection guard. The following values are the
   known placeholders and will be rejected:
   - `JWT_SECRET = "dev-jwt-secret-change-in-prod-0123456789abcdef"`
   - `AES_256_KEY_HEX = "0123456789abcdef…" ` (the 32-byte repeating value
     shipped in `docker-compose.yml`)
   - `DEFAULT_ADMIN_PASSWORD ∈ { "admin", "admin123", "password", "123456" }`
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
5. **Rotate `AES_256_KEY_HEX` carefully** — previously encrypted values
   (`users.home_address_enc`) are unrecoverable after a rotation unless you
   write a migration that re-encrypts under the new key.

## Offline ZIP+4 normalization

Address geocoding runs entirely offline against a bundled index at
`backend/data/zip4_index.csv`. The sample file covers representative US
cities; swap in a real USPS-derived dataset before go-live. The file is
embedded at compile time via `include_str!`, so a rebuild is required
after updating it.
