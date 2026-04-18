-- FieldOps Kitchen & Training Console — schema per PRD §4.
-- Migrations are applied in order on backend boot when RUN_MIGRATIONS_ON_BOOT=true.

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- -----------------------------------------------------------------------------
-- Enums
-- -----------------------------------------------------------------------------
DO $$ BEGIN
    CREATE TYPE user_role AS ENUM ('TECH', 'SUPER', 'ADMIN');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE work_order_priority AS ENUM ('LOW', 'NORMAL', 'HIGH', 'CRITICAL');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE work_order_state AS ENUM (
        'Scheduled', 'EnRoute', 'OnSite', 'InProgress',
        'WaitingOnParts', 'Completed', 'Canceled'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE timer_alert_type AS ENUM ('AUDIBLE', 'VISUAL', 'BOTH');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE step_progress_status AS ENUM (
        'Pending', 'InProgress', 'Paused', 'Completed'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE check_in_type AS ENUM ('ARRIVAL', 'DEPARTURE');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE notification_template_type AS ENUM (
        'SIGNUP_SUCCESS', 'SCHEDULE_CHANGE', 'CANCELLATION', 'REVIEW_RESULT'
    );
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE sync_operation AS ENUM ('INSERT', 'UPDATE', 'DELETE');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- -----------------------------------------------------------------------------
-- branches
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS branches (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                 VARCHAR NOT NULL,
    address              VARCHAR,
    lat                  DOUBLE PRECISION,
    lng                  DOUBLE PRECISION,
    service_radius_miles INT NOT NULL DEFAULT 30
);

-- -----------------------------------------------------------------------------
-- users
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS users (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username         VARCHAR UNIQUE NOT NULL,
    password_hash    VARCHAR NOT NULL,
    role             user_role NOT NULL,
    branch_id        UUID REFERENCES branches(id),
    full_name        VARCHAR,
    home_address_enc TEXT,
    privacy_mode     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at       TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_users_role       ON users(role);
CREATE INDEX IF NOT EXISTS idx_users_branch     ON users(branch_id);
CREATE INDEX IF NOT EXISTS idx_users_deleted_at ON users(deleted_at);

-- -----------------------------------------------------------------------------
-- recipes
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS recipes (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name         VARCHAR NOT NULL,
    description  TEXT,
    created_by   UUID REFERENCES users(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- -----------------------------------------------------------------------------
-- recipe_steps
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS recipe_steps (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recipe_id    UUID NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
    step_order   INT NOT NULL,
    title        VARCHAR NOT NULL,
    instructions TEXT,
    is_pauseable BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE (recipe_id, step_order)
);
CREATE INDEX IF NOT EXISTS idx_recipe_steps_recipe ON recipe_steps(recipe_id);

-- -----------------------------------------------------------------------------
-- step_timers
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS step_timers (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    step_id          UUID NOT NULL REFERENCES recipe_steps(id) ON DELETE CASCADE,
    label            VARCHAR NOT NULL,
    duration_seconds INT NOT NULL,
    alert_type       timer_alert_type NOT NULL DEFAULT 'BOTH'
);
CREATE INDEX IF NOT EXISTS idx_step_timers_step ON step_timers(step_id);

-- -----------------------------------------------------------------------------
-- tip_cards
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS tip_cards (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    step_id      UUID NOT NULL REFERENCES recipe_steps(id) ON DELETE CASCADE,
    title        VARCHAR NOT NULL,
    content      TEXT NOT NULL,
    authored_by  UUID REFERENCES users(id),
    is_pinned    BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_tip_cards_step ON tip_cards(step_id);

-- -----------------------------------------------------------------------------
-- work_orders
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS work_orders (
    id                        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title                     VARCHAR NOT NULL,
    description               TEXT,
    priority                  work_order_priority NOT NULL DEFAULT 'NORMAL',
    state                     work_order_state NOT NULL DEFAULT 'Scheduled',
    assigned_tech_id          UUID REFERENCES users(id),
    branch_id                 UUID REFERENCES branches(id),
    sla_deadline              TIMESTAMPTZ,
    recipe_id                 UUID REFERENCES recipes(id),
    location_address_norm     VARCHAR,
    location_lat              DOUBLE PRECISION,
    location_lng              DOUBLE PRECISION,
    etag                      VARCHAR,
    version_count             INT NOT NULL DEFAULT 1,
    created_at                TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at                TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at                TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_work_orders_tech     ON work_orders(assigned_tech_id);
CREATE INDEX IF NOT EXISTS idx_work_orders_branch   ON work_orders(branch_id);
CREATE INDEX IF NOT EXISTS idx_work_orders_state    ON work_orders(state);
CREATE INDEX IF NOT EXISTS idx_work_orders_priority ON work_orders(priority);
CREATE INDEX IF NOT EXISTS idx_work_orders_deleted  ON work_orders(deleted_at);

-- -----------------------------------------------------------------------------
-- work_order_transitions  (immutable)
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS work_order_transitions (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    work_order_id    UUID NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    from_state       VARCHAR,
    to_state         VARCHAR NOT NULL,
    triggered_by     UUID REFERENCES users(id),
    required_fields  JSONB NOT NULL DEFAULT '{}'::jsonb,
    notes            TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_wot_wo ON work_order_transitions(work_order_id);

-- Enforce immutability: no UPDATE or DELETE allowed.
CREATE OR REPLACE FUNCTION wot_immutable() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'work_order_transitions is immutable';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_wot_no_update ON work_order_transitions;
CREATE TRIGGER trg_wot_no_update
    BEFORE UPDATE OR DELETE ON work_order_transitions
    FOR EACH ROW EXECUTE FUNCTION wot_immutable();

-- -----------------------------------------------------------------------------
-- job_step_progress
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS job_step_progress (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    work_order_id        UUID NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    step_id              UUID NOT NULL REFERENCES recipe_steps(id),
    status               step_progress_status NOT NULL DEFAULT 'Pending',
    started_at           TIMESTAMPTZ,
    paused_at            TIMESTAMPTZ,
    completed_at         TIMESTAMPTZ,
    notes                TEXT,
    timer_state_snapshot JSONB,
    etag                 VARCHAR,
    version              INT NOT NULL DEFAULT 1,
    UNIQUE (work_order_id, step_id)
);
CREATE INDEX IF NOT EXISTS idx_jsp_wo   ON job_step_progress(work_order_id);
CREATE INDEX IF NOT EXISTS idx_jsp_step ON job_step_progress(step_id);

-- -----------------------------------------------------------------------------
-- job_step_progress_versions  (max 30 enforced at app layer)
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS job_step_progress_versions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    progress_id  UUID NOT NULL REFERENCES job_step_progress(id) ON DELETE CASCADE,
    snapshot     JSONB NOT NULL,
    version      INT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (progress_id, version)
);
CREATE INDEX IF NOT EXISTS idx_jspv_progress ON job_step_progress_versions(progress_id);

-- -----------------------------------------------------------------------------
-- location_trails
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS location_trails (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    work_order_id     UUID NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    user_id           UUID NOT NULL REFERENCES users(id),
    lat               DOUBLE PRECISION NOT NULL,
    lng               DOUBLE PRECISION NOT NULL,
    precision_reduced BOOLEAN NOT NULL DEFAULT FALSE,
    recorded_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_loc_wo   ON location_trails(work_order_id);
CREATE INDEX IF NOT EXISTS idx_loc_user ON location_trails(user_id);

-- -----------------------------------------------------------------------------
-- check_ins
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS check_ins (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    work_order_id  UUID NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    user_id        UUID NOT NULL REFERENCES users(id),
    type           check_in_type NOT NULL,
    lat            DOUBLE PRECISION,
    lng            DOUBLE PRECISION,
    recorded_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_checkins_wo ON check_ins(work_order_id);

-- -----------------------------------------------------------------------------
-- knowledge_points
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS knowledge_points (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    recipe_id           UUID NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
    step_id             UUID REFERENCES recipe_steps(id) ON DELETE SET NULL,
    title               VARCHAR NOT NULL,
    content             TEXT,
    quiz_question       TEXT,
    quiz_options        JSONB,
    quiz_correct_answer VARCHAR
);
CREATE INDEX IF NOT EXISTS idx_kp_recipe ON knowledge_points(recipe_id);

-- -----------------------------------------------------------------------------
-- learning_records
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS learning_records (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             UUID NOT NULL REFERENCES users(id),
    knowledge_point_id  UUID NOT NULL REFERENCES knowledge_points(id),
    work_order_id       UUID REFERENCES work_orders(id) ON DELETE SET NULL,
    quiz_score          DOUBLE PRECISION,
    time_spent_seconds  INT,
    review_count        INT NOT NULL DEFAULT 0,
    completed_at        TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_lr_user      ON learning_records(user_id);
CREATE INDEX IF NOT EXISTS idx_lr_completed ON learning_records(completed_at);

-- -----------------------------------------------------------------------------
-- notifications
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS notifications (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id),
    template_type   notification_template_type NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    delivered_at    TIMESTAMPTZ,
    read_at         TIMESTAMPTZ,
    retry_count     INT NOT NULL DEFAULT 0,
    is_unsubscribed BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_notif_user         ON notifications(user_id);
CREATE INDEX IF NOT EXISTS idx_notif_created      ON notifications(created_at);
CREATE INDEX IF NOT EXISTS idx_notif_template     ON notifications(template_type);

-- Per-user, per-template unsubscribe preferences.
CREATE TABLE IF NOT EXISTS notification_unsubscribes (
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    template_type notification_template_type NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, template_type)
);

-- -----------------------------------------------------------------------------
-- sync_log
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sync_log (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_table          VARCHAR NOT NULL,
    entity_id             UUID NOT NULL,
    operation             sync_operation NOT NULL,
    old_etag              VARCHAR,
    new_etag              VARCHAR,
    synced_at             TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    conflict_flagged      BOOLEAN NOT NULL DEFAULT FALSE,
    conflict_resolved_by  UUID REFERENCES users(id)
);
CREATE INDEX IF NOT EXISTS idx_sync_entity   ON sync_log(entity_table, entity_id);
CREATE INDEX IF NOT EXISTS idx_sync_conflict ON sync_log(conflict_flagged);
