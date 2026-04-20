-- Append-only processing log: every state-changing user action writes a row
-- here so the system has a single immutable audit trail spanning the whole
-- product (PRD §7 "immutable processing log for every user action").
--
-- Distinct from `work_order_transitions` (which only records state moves
-- of a work order) -- this table also carries step-progress changes,
-- check-ins, trail points, notification read receipts, auth events, and
-- admin mutations. The set of actions is open; `action` is a free-form
-- identifier owned by the writing module.

CREATE TABLE IF NOT EXISTS processing_log (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID REFERENCES users(id),
    action        VARCHAR NOT NULL,
    entity_table  VARCHAR NOT NULL,
    entity_id     UUID,
    payload       JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_plog_entity  ON processing_log(entity_table, entity_id);
CREATE INDEX IF NOT EXISTS idx_plog_user    ON processing_log(user_id);
CREATE INDEX IF NOT EXISTS idx_plog_action  ON processing_log(action);
CREATE INDEX IF NOT EXISTS idx_plog_created ON processing_log(created_at);

CREATE OR REPLACE FUNCTION plog_immutable() RETURNS trigger AS $$
BEGIN
    -- Retention pruning may delete old rows; nothing else may.
    IF current_setting('fieldops.retention_prune', TRUE) = 'on' AND TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'processing_log is immutable';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_plog_no_update ON processing_log;
CREATE TRIGGER trg_plog_no_update
    BEFORE UPDATE OR DELETE ON processing_log
    FOR EACH ROW EXECUTE FUNCTION plog_immutable();
