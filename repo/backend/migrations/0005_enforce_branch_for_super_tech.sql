-- Fail-closed tenant isolation (PRD §6 security rule): SUPER and TECH
-- principals MUST be pinned to a branch. The scope SQL used to treat
-- `branch_id IS NULL` as "unscoped, see everything", which let any SUPER
-- row without a branch quietly widen its visibility across the tenant.
-- This check is the database-level half of the fix; the API layer also
-- rejects the same shape, but the constraint keeps the invariant true
-- even if a future code path bypasses the handler validation.

ALTER TABLE users
    ADD CONSTRAINT users_branch_required_for_super_tech
    CHECK (
        role = 'ADMIN' OR branch_id IS NOT NULL
    ) NOT VALID;

-- Validate separately so an in-place migration doesn't block on legacy
-- rows that predate the rule; operators must clean those up explicitly
-- and then re-run `ALTER TABLE users VALIDATE CONSTRAINT ...`.
ALTER TABLE users
    VALIDATE CONSTRAINT users_branch_required_for_super_tech;
