//! Validates the sync-log conflict counter surfaces unresolved rows and
//! ignores resolved ones — matches PRD §8 "never overwrite; block until SUPER
//! resolves" invariant at the reporting layer.

use fieldops_backend::config::AppConfig;
use fieldops_backend::{db, sync};
use sqlx::PgPool;

async fn fresh() -> PgPool {
    let cfg = AppConfig::test();
    let pool = PgPool::connect(&cfg.database.url).await.unwrap();
    db::run_migrations(&pool).await.unwrap();
    db::truncate_all(&pool).await.unwrap();
    pool
}

#[actix_web::test]
async fn trigger_reports_zero_conflicts_on_clean_db() {
    let pool = fresh().await;
    let r = sync::trigger(&pool).await.unwrap();
    assert_eq!(r.conflicts_flagged, 0);
    assert_eq!(r.work_orders_updated, 0);
}

#[actix_web::test]
async fn trigger_counts_unresolved_conflicts() {
    let pool = fresh().await;
    // Seed one resolved and two unresolved conflict rows.
    let resolver: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role)
         VALUES ('super', 'x', 'SUPER') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    for _ in 0..2 {
        sqlx::query(
            "INSERT INTO sync_log
                (entity_table, entity_id, operation, conflict_flagged)
             VALUES ('job_step_progress', $1, 'UPDATE', TRUE)",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO sync_log
            (entity_table, entity_id, operation, conflict_flagged, conflict_resolved_by)
         VALUES ('job_step_progress', $1, 'UPDATE', TRUE, $2)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(resolver)
    .execute(&pool)
    .await
    .unwrap();

    let r = sync::trigger(&pool).await.unwrap();
    assert_eq!(r.conflicts_flagged, 2, "resolved conflicts should be excluded");
}

#[actix_web::test]
async fn merge_applies_fresh_insert() {
    use fieldops_backend::enums::StepProgressStatus;
    use fieldops_backend::sync::merge::{merge_step_progress, IncomingProgress, MergeOutcome};

    let pool = fresh().await;
    // Minimal seed: branch/user/recipe/step/work_order.
    let branch: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO branches (name, lat, lng) VALUES ('B', 37.0, -122.0) RETURNING id",
    )
    .fetch_one(&pool).await.unwrap();
    let tech: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, branch_id)
         VALUES ('tech', 'x', 'TECH', $1) RETURNING id",
    ).bind(branch).fetch_one(&pool).await.unwrap();
    let recipe: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO recipes (name) VALUES ('R') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let step: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO recipe_steps (recipe_id, step_order, title) VALUES ($1, 1, 'S') RETURNING id",
    ).bind(recipe).fetch_one(&pool).await.unwrap();
    let wo: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO work_orders (title, priority, state, assigned_tech_id, branch_id, recipe_id, version_count)
         VALUES ('WO','NORMAL','OnSite',$1,$2,$3,1) RETURNING id",
    ).bind(tech).bind(branch).bind(recipe).fetch_one(&pool).await.unwrap();

    let inc = IncomingProgress {
        work_order_id: wo,
        step_id: step,
        status: StepProgressStatus::InProgress,
        notes: Some("started".into()),
        timer_state_snapshot: None,
        version: 1,
        updated_at: chrono::Utc::now(),
    };
    assert_eq!(merge_step_progress(&pool, &inc).await.unwrap(), MergeOutcome::Applied);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM job_step_progress WHERE work_order_id = $1 AND step_id = $2",
    ).bind(wo).bind(step).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1);
}

#[actix_web::test]
async fn merge_rejects_overwrite_of_completed_and_flags_conflict() {
    use fieldops_backend::enums::StepProgressStatus;
    use fieldops_backend::sync::merge::{merge_step_progress, IncomingProgress, MergeOutcome};

    let pool = fresh().await;
    let branch: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO branches (name, lat, lng) VALUES ('B', 37.0, -122.0) RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let tech: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, branch_id)
         VALUES ('tech', 'x', 'TECH', $1) RETURNING id",
    ).bind(branch).fetch_one(&pool).await.unwrap();
    let recipe: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO recipes (name) VALUES ('R') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let step: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO recipe_steps (recipe_id, step_order, title) VALUES ($1, 1, 'S') RETURNING id",
    ).bind(recipe).fetch_one(&pool).await.unwrap();
    let wo: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO work_orders (title, priority, state, assigned_tech_id, branch_id, recipe_id, version_count)
         VALUES ('WO','NORMAL','InProgress',$1,$2,$3,1) RETURNING id",
    ).bind(tech).bind(branch).bind(recipe).fetch_one(&pool).await.unwrap();

    // Local row already Completed — this log is immutable.
    sqlx::query(
        "INSERT INTO job_step_progress (work_order_id, step_id, status, notes, version)
         VALUES ($1,$2,'Completed','final note',3)",
    ).bind(wo).bind(step).execute(&pool).await.unwrap();

    // Incoming replica tries to mutate it back to InProgress.
    let inc = IncomingProgress {
        work_order_id: wo,
        step_id: step,
        status: StepProgressStatus::InProgress,
        notes: Some("replica thinks it's still running".into()),
        timer_state_snapshot: None,
        version: 4,
        updated_at: chrono::Utc::now(),
    };
    let outcome = merge_step_progress(&pool, &inc).await.unwrap();
    assert_eq!(outcome, MergeOutcome::Conflict);

    // Local status unchanged.
    let status: String = sqlx::query_scalar(
        "SELECT status::text FROM job_step_progress WHERE work_order_id = $1 AND step_id = $2",
    ).bind(wo).bind(step).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "Completed");

    // A flagged conflict landed in sync_log.
    let flagged: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log
         WHERE entity_id = $1 AND conflict_flagged = TRUE AND conflict_resolved_by IS NULL",
    ).bind(step).fetch_one(&pool).await.unwrap();
    assert_eq!(flagged, 1);

    // And the original notes are preserved, with the replica's extra appended.
    let notes: String = sqlx::query_scalar(
        "SELECT notes FROM job_step_progress WHERE work_order_id = $1 AND step_id = $2",
    ).bind(wo).bind(step).fetch_one(&pool).await.unwrap();
    assert!(notes.contains("final note"));
    assert!(notes.contains("replica thinks"));
}

#[actix_web::test]
async fn merge_higher_version_wins_deterministically() {
    use fieldops_backend::enums::StepProgressStatus;
    use fieldops_backend::sync::merge::{merge_step_progress, IncomingProgress, MergeOutcome};

    let pool = fresh().await;
    let branch: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO branches (name, lat, lng) VALUES ('B', 37.0, -122.0) RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let tech: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, branch_id)
         VALUES ('tech', 'x', 'TECH', $1) RETURNING id",
    ).bind(branch).fetch_one(&pool).await.unwrap();
    let recipe: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO recipes (name) VALUES ('R') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let step: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO recipe_steps (recipe_id, step_order, title) VALUES ($1, 1, 'S') RETURNING id",
    ).bind(recipe).fetch_one(&pool).await.unwrap();
    let wo: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO work_orders (title, priority, state, assigned_tech_id, branch_id, recipe_id, version_count)
         VALUES ('WO','NORMAL','InProgress',$1,$2,$3,1) RETURNING id",
    ).bind(tech).bind(branch).bind(recipe).fetch_one(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO job_step_progress (work_order_id, step_id, status, notes, version)
         VALUES ($1,$2,'InProgress','v1',1)",
    ).bind(wo).bind(step).execute(&pool).await.unwrap();

    // Older incoming is rejected.
    let older = IncomingProgress {
        work_order_id: wo, step_id: step,
        status: StepProgressStatus::Paused,
        notes: Some("v0-paused".into()),
        timer_state_snapshot: None,
        version: 0,
        updated_at: chrono::Utc::now(),
    };
    assert_eq!(merge_step_progress(&pool, &older).await.unwrap(), MergeOutcome::RejectedOlder);
    let status: String = sqlx::query_scalar(
        "SELECT status::text FROM job_step_progress WHERE work_order_id = $1 AND step_id = $2",
    ).bind(wo).bind(step).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "InProgress");

    // Newer incoming wins.
    let newer = IncomingProgress {
        work_order_id: wo, step_id: step,
        status: StepProgressStatus::Paused,
        notes: Some("v2-paused".into()),
        timer_state_snapshot: None,
        version: 2,
        updated_at: chrono::Utc::now(),
    };
    assert_eq!(merge_step_progress(&pool, &newer).await.unwrap(), MergeOutcome::Applied);
    let status: String = sqlx::query_scalar(
        "SELECT status::text FROM job_step_progress WHERE work_order_id = $1 AND step_id = $2",
    ).bind(wo).bind(step).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "Paused");
}

#[actix_web::test]
async fn merge_equal_version_conflict_flagged_for_super() {
    use fieldops_backend::enums::StepProgressStatus;
    use fieldops_backend::sync::merge::{merge_step_progress, IncomingProgress, MergeOutcome};

    let pool = fresh().await;
    let branch: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO branches (name, lat, lng) VALUES ('B', 37.0, -122.0) RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let tech: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, branch_id)
         VALUES ('tech', 'x', 'TECH', $1) RETURNING id",
    ).bind(branch).fetch_one(&pool).await.unwrap();
    let recipe: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO recipes (name) VALUES ('R') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let step: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO recipe_steps (recipe_id, step_order, title) VALUES ($1, 1, 'S') RETURNING id",
    ).bind(recipe).fetch_one(&pool).await.unwrap();
    let wo: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO work_orders (title, priority, state, assigned_tech_id, branch_id, recipe_id, version_count)
         VALUES ('WO','NORMAL','InProgress',$1,$2,$3,1) RETURNING id",
    ).bind(tech).bind(branch).bind(recipe).fetch_one(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO job_step_progress (work_order_id, step_id, status, notes, version)
         VALUES ($1,$2,'InProgress','from device A',2)",
    ).bind(wo).bind(step).execute(&pool).await.unwrap();

    let incoming = IncomingProgress {
        work_order_id: wo, step_id: step,
        status: StepProgressStatus::Paused,
        notes: Some("from device B".into()),
        timer_state_snapshot: None,
        version: 2,
        updated_at: chrono::Utc::now(),
    };
    assert_eq!(merge_step_progress(&pool, &incoming).await.unwrap(), MergeOutcome::Conflict);
    let flagged: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_log
         WHERE entity_id = $1 AND conflict_flagged = TRUE AND conflict_resolved_by IS NULL",
    ).bind(step).fetch_one(&pool).await.unwrap();
    assert_eq!(flagged, 1);
}

#[actix_web::test]
async fn trigger_writes_sync_log_on_etag_change() {
    let pool = fresh().await;
    sqlx::query(
        "INSERT INTO work_orders (title, priority, state, version_count)
         VALUES ('WO', 'NORMAL', 'Scheduled', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // First trigger — no etag, so it will be computed + logged.
    let r1 = sync::trigger(&pool).await.unwrap();
    assert_eq!(r1.work_orders_updated, 1);

    // Second trigger — etag is now stable, no new log rows.
    let r2 = sync::trigger(&pool).await.unwrap();
    assert_eq!(r2.work_orders_updated, 0);

    let log_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sync_log WHERE entity_table = 'work_orders'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(log_count, 1);
}
