-- Automatic on-call routing flag (PRD §7 / audit-2 High #2):
-- "dispatch to on-call queue when priority=HIGH and SLA deadline is within
-- ON_CALL_HIGH_PRIORITY_HOURS" is persisted as a durable column so it is an
-- actual routing decision, not a read-time filter recomputed on every query.
-- The backend re-evaluates the flag on create and on state transition and
-- writes a processing_log row whenever it flips.

ALTER TABLE work_orders
    ADD COLUMN IF NOT EXISTS on_call BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_work_orders_on_call ON work_orders(on_call)
    WHERE on_call = TRUE;
