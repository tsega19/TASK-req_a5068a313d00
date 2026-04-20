-- Adds `updated_at` to job_step_progress so the merge policy (PRD §8)
-- can apply a deterministic timestamp-priority tiebreaker on equal-version
-- conflicts. The column is NOT NULL with a DEFAULT so existing rows inherit
-- a non-null anchor at migration time.

ALTER TABLE job_step_progress
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

CREATE INDEX IF NOT EXISTS idx_jsp_updated_at ON job_step_progress(updated_at);
