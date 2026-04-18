-- Security-hardening migration.
-- Adds:
--   - users.password_reset_required: when true, the user must change their
--     password before the client considers the session fully authenticated.
--     Set on first boot for the seeded default admin when
--     REQUIRE_ADMIN_PASSWORD_CHANGE=true.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS password_reset_required BOOLEAN NOT NULL DEFAULT FALSE;
