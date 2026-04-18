//! Admin panel endpoints: user + branch management, sync trigger.
//! All routes require role=ADMIN (enforced twice — middleware wrap + per-handler
//! `require_role(Role::Admin)`).

use actix_web::{delete, get, post, put, web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::hashing::hash_password;
use crate::auth::models::{Role, UserRow};
use crate::config::AppConfig;
use crate::errors::ApiError;
use crate::middleware::rbac::{require_role, AuthedUser};
use crate::pagination::{PageParams, Paginated};
use crate::sync;
use crate::log_info;

const MODULE: &str = "admin";

// -----------------------------------------------------------------------------
// Users
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
    pub role: Role,
    pub branch_id: Option<Uuid>,
    pub full_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUser {
    pub password: Option<String>,
    pub role: Option<Role>,
    pub branch_id: Option<Uuid>,
    pub full_name: Option<String>,
    pub privacy_mode: Option<bool>,
}

#[get("/users")]
pub async fn list_users(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<PageParams>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let params = q.into_inner();
    let (offset, limit) = params.offset_limit();
    let rows = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, branch_id, full_name, privacy_mode,
                password_reset_required
         FROM users WHERE deleted_at IS NULL
         ORDER BY username ASC OFFSET $1 LIMIT $2",
    )
    .bind(offset)
    .bind(limit)
    .fetch_all(pool.get_ref())
    .await?;
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE deleted_at IS NULL")
            .fetch_one(pool.get_ref())
            .await?;
    log_info!(MODULE, "users_list", "user={} count={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok().json(Paginated::new(rows, params, total)))
}

#[post("/users")]
pub async fn create_user(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
    body: web::Json<CreateUser>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let req = body.into_inner();
    if req.username.trim().is_empty() || req.password.len() < 4 {
        return Err(ApiError::BadRequest("username + password (≥4 chars) required".into()));
    }
    let hash = hash_password(&req.password, &cfg.auth)?;
    let row = sqlx::query_as::<_, UserRow>(
        "INSERT INTO users (username, password_hash, role, branch_id, full_name)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, username, password_hash, role, branch_id, full_name, privacy_mode,
                password_reset_required",
    )
    .bind(&req.username)
    .bind(&hash)
    .bind(req.role)
    .bind(req.branch_id)
    .bind(&req.full_name)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|e| {
        if e.to_string().contains("users_username_key") {
            ApiError::Conflict("username already exists".into())
        } else {
            e.into()
        }
    })?;
    log_info!(MODULE, "users_create", "actor={} new_user={} role={}", user.user_id(), row.id, row.role);
    Ok(HttpResponse::Created().json(row))
}

#[put("/users/{id}")]
pub async fn update_user(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateUser>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let id = path.into_inner();
    let req = body.into_inner();
    let new_hash = match &req.password {
        Some(p) if !p.is_empty() => Some(hash_password(p, &cfg.auth)?),
        _ => None,
    };
    let row = sqlx::query_as::<_, UserRow>(
        "UPDATE users SET
            password_hash = COALESCE($1, password_hash),
            role          = COALESCE($2, role),
            branch_id     = COALESCE($3, branch_id),
            full_name     = COALESCE($4, full_name),
            privacy_mode  = COALESCE($5, privacy_mode),
            updated_at    = NOW()
         WHERE id = $6 AND deleted_at IS NULL
         RETURNING id, username, password_hash, role, branch_id, full_name, privacy_mode,
                password_reset_required",
    )
    .bind(new_hash)
    .bind(req.role)
    .bind(req.branch_id)
    .bind(&req.full_name)
    .bind(req.privacy_mode)
    .bind(id)
    .fetch_optional(pool.get_ref())
    .await?;
    let row = row.ok_or_else(|| ApiError::NotFound("user not found".into()))?;
    log_info!(MODULE, "users_update", "actor={} target={}", user.user_id(), id);
    Ok(HttpResponse::Ok().json(row))
}

#[delete("/users/{id}")]
pub async fn delete_user(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let id = path.into_inner();
    if id == user.user_id() {
        return Err(ApiError::BadRequest("admins cannot delete themselves".into()));
    }
    let affected = sqlx::query(
        "UPDATE users SET deleted_at = NOW(), updated_at = NOW()
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(pool.get_ref())
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound("user not found".into()));
    }
    log_info!(MODULE, "users_delete", "actor={} target={} soft-deleted", user.user_id(), id);
    Ok(HttpResponse::NoContent().finish())
}

// -----------------------------------------------------------------------------
// Branches
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Branch {
    pub id: Uuid,
    pub name: String,
    pub address: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub service_radius_miles: i32,
}

#[derive(Debug, Deserialize)]
pub struct CreateBranch {
    pub name: String,
    pub address: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub service_radius_miles: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBranch {
    pub name: Option<String>,
    pub address: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub service_radius_miles: Option<i32>,
}

#[get("/branches")]
pub async fn list_branches(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<PageParams>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let params = q.into_inner();
    let (offset, limit) = params.offset_limit();
    let rows = sqlx::query_as::<_, Branch>(
        "SELECT * FROM branches ORDER BY name ASC OFFSET $1 LIMIT $2",
    )
    .bind(offset)
    .bind(limit)
    .fetch_all(pool.get_ref())
    .await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM branches")
        .fetch_one(pool.get_ref())
        .await?;
    log_info!(MODULE, "branches_list", "user={} count={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok().json(Paginated::new(rows, params, total)))
}

#[post("/branches")]
pub async fn create_branch(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
    body: web::Json<CreateBranch>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let req = body.into_inner();
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name required".into()));
    }
    let radius = req
        .service_radius_miles
        .unwrap_or(cfg.business.default_service_radius_miles);
    let row = sqlx::query_as::<_, Branch>(
        "INSERT INTO branches (name, address, lat, lng, service_radius_miles)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(&req.name)
    .bind(&req.address)
    .bind(req.lat)
    .bind(req.lng)
    .bind(radius)
    .fetch_one(pool.get_ref())
    .await?;
    log_info!(MODULE, "branches_create", "actor={} branch={}", user.user_id(), row.id);
    Ok(HttpResponse::Created().json(row))
}

#[put("/branches/{id}")]
pub async fn update_branch(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateBranch>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let id = path.into_inner();
    let req = body.into_inner();
    let row = sqlx::query_as::<_, Branch>(
        "UPDATE branches SET
            name                 = COALESCE($1, name),
            address              = COALESCE($2, address),
            lat                  = COALESCE($3, lat),
            lng                  = COALESCE($4, lng),
            service_radius_miles = COALESCE($5, service_radius_miles)
         WHERE id = $6
         RETURNING *",
    )
    .bind(&req.name)
    .bind(&req.address)
    .bind(req.lat)
    .bind(req.lng)
    .bind(req.service_radius_miles)
    .bind(id)
    .fetch_optional(pool.get_ref())
    .await?;
    let row = row.ok_or_else(|| ApiError::NotFound("branch not found".into()))?;
    log_info!(MODULE, "branches_update", "actor={} branch={}", user.user_id(), id);
    Ok(HttpResponse::Ok().json(row))
}

// -----------------------------------------------------------------------------
// Sync trigger
// -----------------------------------------------------------------------------

#[post("/sync/trigger")]
pub async fn trigger_sync(
    user: AuthedUser,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let report = sync::trigger(pool.get_ref()).await?;
    log_info!(MODULE, "sync_trigger", "actor={} conflicts={}", user.user_id(), report.conflicts_flagged);
    Ok(HttpResponse::Ok().json(report.to_json()))
}

#[post("/retention/prune")]
pub async fn trigger_retention(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let report = crate::retention::prune(pool.get_ref(), cfg.get_ref()).await?;
    log_info!(
        MODULE,
        "retention_prune",
        "actor={} users={} work_orders={}",
        user.user_id(),
        report.users_pruned,
        report.work_orders_pruned
    );
    Ok(HttpResponse::Ok().json(report.to_json()))
}

#[post("/notifications/retry")]
pub async fn trigger_notifications_retry(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let report = crate::notifications::stub::retry_pending(pool.get_ref(), cfg.get_ref()).await?;
    log_info!(
        MODULE,
        "notifications_retry",
        "actor={} scanned={} delivered={} giveup={}",
        user.user_id(),
        report.scanned,
        report.delivered,
        report.giveup
    );
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "scanned": report.scanned,
        "delivered": report.delivered,
        "giveup": report.giveup,
        "skipped_backoff": report.skipped_backoff,
    })))
}

// -----------------------------------------------------------------------------
// Scope wiring
// -----------------------------------------------------------------------------
pub fn scope() -> actix_web::Scope {
    web::scope("/api/admin")
        .service(list_users)
        .service(create_user)
        .service(update_user)
        .service(delete_user)
        .service(list_branches)
        .service(create_branch)
        .service(update_branch)
        .service(trigger_sync)
        .service(trigger_retention)
        .service(trigger_notifications_retry)
}
