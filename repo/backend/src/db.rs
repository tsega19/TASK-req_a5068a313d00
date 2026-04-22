//! Database bootstrap: pool creation, migration application, default-admin seed.

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Executor, Row};
use std::path::Path;
use std::time::Duration;

use crate::auth::hashing::hash_password;
use crate::config::AppConfig;
use crate::{log_error, log_info, log_warn};

pub async fn connect(cfg: &AppConfig) -> anyhow::Result<PgPool> {
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        match PgPoolOptions::new()
            .max_connections(cfg.database.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&cfg.database.url)
            .await
        {
            Ok(pool) => {
                log_info!("db", "connect", "connected after {} attempt(s)", attempts);
                return Ok(pool);
            }
            Err(e) if attempts < 30 => {
                log_warn!("db", "connect", "attempt {} failed: {}; retrying", attempts, e);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) => {
                log_error!("db", "connect", "giving up after {} attempts: {}", attempts, e);
                return Err(e.into());
            }
        }
    }
}

/// Apply every `*.sql` file under `migrations/` in lexicographic order.
/// Each file runs as a single batched statement.
pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    let dir = Path::new("migrations");
    if !dir.exists() {
        log_warn!("db", "migrate", "no migrations directory found");
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("sql"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let sql = std::fs::read_to_string(&path)?;
        log_info!("db", "migrate", "applying {}", name);
        // Use `Executor::execute(&str)` rather than `sqlx::query(...)` — the
        // former uses Postgres's simple query protocol which accepts multi-
        // statement bodies. `sqlx::query` goes through prepared statements,
        // which Postgres rejects with "cannot insert multiple commands into
        // a prepared statement" for our DDL files.
        pool.execute(sql.as_str()).await?;
    }
    Ok(())
}

pub async fn seed_default_admin(pool: &PgPool, cfg: &AppConfig) -> anyhow::Result<()> {
    if !cfg.app.seed_default_admin {
        return Ok(());
    }
    let existing: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT id FROM users WHERE username = $1")
            .bind(&cfg.app.default_admin_username)
            .fetch_optional(pool)
            .await?;
    if existing.is_some() {
        log_info!("db", "seed", "default admin already present");
        return Ok(());
    }
    let hash = hash_password(&cfg.app.default_admin_password, &cfg.auth)?;
    // When password rotation is required, mark the row — the auth flow
    // surfaces this to the client so the admin is forced to change their
    // password before doing anything sensitive.
    let reset_required = cfg.app.require_admin_password_change;
    sqlx::query(
        "INSERT INTO users (username, password_hash, role, full_name, password_reset_required)
         VALUES ($1, $2, 'ADMIN', 'Default Admin', $3)",
    )
    .bind(&cfg.app.default_admin_username)
    .bind(&hash)
    .bind(reset_required)
    .execute(pool)
    .await?;
    log_info!(
        "db",
        "seed",
        "inserted default admin '{}' (password_reset_required={})",
        cfg.app.default_admin_username,
        reset_required
    );
    Ok(())
}

/// Convenience used by tests and startup health checks.
pub async fn ping(pool: &PgPool) -> anyhow::Result<()> {
    let row = sqlx::query("SELECT 1 AS one").fetch_one(pool).await?;
    let _: i32 = row.try_get("one")?;
    Ok(())
}

/// Truncate every mutable table. Used by the integration test harness to
/// reset state between suites. Preserves the schema, enums, and triggers
/// (which are created by the migrations). The `work_order_transitions`
/// table has an immutable trigger — we TRUNCATE (DDL) which bypasses it.
pub async fn truncate_all(pool: &PgPool) -> anyhow::Result<()> {
    // `record_versions` is append-only with a DB trigger that blocks
    // DELETE/UPDATE. TRUNCATE is a DDL-level wipe and bypasses row-level
    // triggers, so it's safe here — tests rely on per-suite truncation
    // to start from a clean slate.
    let stmt = "TRUNCATE TABLE
        notification_unsubscribes,
        notifications,
        learning_records,
        knowledge_points,
        check_ins,
        location_trails,
        job_step_progress_versions,
        job_step_progress,
        record_versions,
        processing_log,
        work_order_transitions,
        work_orders,
        tip_cards,
        step_timers,
        recipe_steps,
        recipes,
        sync_log,
        users,
        branches
        RESTART IDENTITY CASCADE";
    pool.execute(stmt).await?;
    Ok(())
}
