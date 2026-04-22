-- Generic per-record version history (PRD §7). Previously only
-- `job_step_progress_versions` existed, which scoped historical retention
-- to a single entity type. This table is the system-wide store: every
-- mutable entity that wants version snapshots writes here, keyed by
-- (entity_table, entity_id, version), and the app layer caps the history
-- length uniformly via `MAX_VERSIONS_PER_RECORD`.
--
-- The append-only discipline is enforced by a BEFORE UPDATE/DELETE trigger
-- that mirrors the one on `work_order_transitions` — once a snapshot row
-- has been written, its contents are immutable. Pruning to the cap
-- happens via a DELETE that the trigger allows (the trigger blocks
-- tampering with row contents, not cap-based pruning). We distinguish
-- app-layer pruning from tampering by checking a session-scoped GUC,
-- `app.record_versions_pruning`, which app code sets to `'on'` for the
-- duration of the prune statement and `'off'` everywhere else.

CREATE TABLE IF NOT EXISTS record_versions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_table VARCHAR NOT NULL,
    entity_id    UUID NOT NULL,
    version      INT NOT NULL,
    snapshot     JSONB NOT NULL,
    actor_id     UUID REFERENCES users(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (entity_table, entity_id, version)
);

CREATE INDEX IF NOT EXISTS idx_record_versions_entity
    ON record_versions(entity_table, entity_id);
CREATE INDEX IF NOT EXISTS idx_record_versions_created_at
    ON record_versions(created_at);

-- Append-only enforcement. UPDATEs are always refused. DELETEs are
-- permitted only when the app has set the pruning GUC, so the cap can
-- be honored without opening a back door to arbitrary audit tampering.
CREATE OR REPLACE FUNCTION record_versions_immutable()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'UPDATE' THEN
        RAISE EXCEPTION 'record_versions rows are immutable';
    END IF;
    IF TG_OP = 'DELETE' THEN
        IF coalesce(current_setting('app.record_versions_pruning', true), 'off') <> 'on' THEN
            RAISE EXCEPTION 'record_versions rows may only be pruned via the app cap';
        END IF;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS record_versions_immutable_trg ON record_versions;
CREATE TRIGGER record_versions_immutable_trg
BEFORE UPDATE OR DELETE ON record_versions
FOR EACH ROW EXECUTE FUNCTION record_versions_immutable();
