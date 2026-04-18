-- Allow retention pruning to delete work orders (cascading into the immutable
-- transition log) while still preventing direct UPDATE/DELETE of the log.
-- The pruner sets `fieldops.retention_prune = 'on'` for its transaction; the
-- trigger honours that, and nobody else can bypass immutability.

CREATE OR REPLACE FUNCTION wot_immutable() RETURNS trigger AS $$
BEGIN
    IF current_setting('fieldops.retention_prune', TRUE) = 'on' AND TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'work_order_transitions is immutable';
END;
$$ LANGUAGE plpgsql;
