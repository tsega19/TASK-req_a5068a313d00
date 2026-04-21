-- Enforce tenant-isolation invariant (PRD §9 + audit report AR-1 High):
-- non-ADMIN principals MUST carry a branch assignment. A SUPER or TECH
-- with branch_id = NULL would widen scope predicates to "see everything",
-- breaking team-level isolation. The check fails closed at the schema.
--
-- ADMIN is intentionally allowed to have branch_id = NULL (global scope).

-- Normalize any pre-existing bad rows so the constraint can be validated
-- without a noisy backfill path. If a SUPER/TECH row already exists without
-- a branch, we refuse to hide the data loss — fail loud by demoting the
-- account to ADMIN is WORSE than failing the migration. Instead leave
-- these rows to be caught by NOT VALID and surfaced via VALIDATE.
DO $$ BEGIN
    IF EXISTS (
        SELECT 1 FROM users
        WHERE role IN ('TECH', 'SUPER') AND branch_id IS NULL AND deleted_at IS NULL
    ) THEN
        RAISE NOTICE 'Found % TECH/SUPER user(s) with NULL branch_id; admin must assign a branch before VALIDATE.',
            (SELECT COUNT(*) FROM users WHERE role IN ('TECH', 'SUPER') AND branch_id IS NULL AND deleted_at IS NULL);
    END IF;
END $$;

ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_branch_required_for_scoped_roles;

ALTER TABLE users
    ADD CONSTRAINT users_branch_required_for_scoped_roles
    CHECK (role = 'ADMIN' OR branch_id IS NOT NULL) NOT VALID;

-- Validate immediately in environments that are already clean; swallow the
-- error in dirty environments so the deployment can proceed and operations
-- can fix the offending rows and re-run VALIDATE manually.
DO $$ BEGIN
    ALTER TABLE users VALIDATE CONSTRAINT users_branch_required_for_scoped_roles;
EXCEPTION WHEN check_violation THEN
    RAISE NOTICE 'users_branch_required_for_scoped_roles left NOT VALID; fix null-branch TECH/SUPER rows then run VALIDATE CONSTRAINT.';
END $$;
