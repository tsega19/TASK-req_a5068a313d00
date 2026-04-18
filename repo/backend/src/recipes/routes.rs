//! Recipes, recipe steps, tip cards.
//! - Recipes & steps: readable by any authenticated user.
//! - Tip cards: read by any authenticated user; author/edit ADMIN only.

use actix_web::{get, post, put, web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::models::Role;
use crate::enums::TimerAlertType;
use crate::errors::ApiError;
use crate::middleware::rbac::{require_role, AuthedUser};
use crate::pagination::{PageParams, Paginated};
use crate::log_info;

const MODULE: &str = "recipes";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Recipe {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RecipeStep {
    pub id: Uuid,
    pub recipe_id: Uuid,
    pub step_order: i32,
    pub title: String,
    pub instructions: Option<String>,
    pub is_pauseable: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StepTimer {
    pub id: Uuid,
    pub step_id: Uuid,
    pub label: String,
    pub duration_seconds: i32,
    pub alert_type: TimerAlertType,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TipCard {
    pub id: Uuid,
    pub step_id: Uuid,
    pub title: String,
    pub content: String,
    pub authored_by: Option<Uuid>,
    pub is_pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTipCard {
    pub step_id: Uuid,
    pub title: String,
    pub content: String,
    pub is_pinned: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTipCard {
    pub title: Option<String>,
    pub content: Option<String>,
    pub is_pinned: Option<bool>,
}

// -----------------------------------------------------------------------------
// Recipes
// -----------------------------------------------------------------------------
#[get("")]
pub async fn list_recipes(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<PageParams>,
) -> Result<HttpResponse, ApiError> {
    let params = q.into_inner();
    let (offset, limit) = params.offset_limit();
    let rows = sqlx::query_as::<_, Recipe>(
        "SELECT * FROM recipes ORDER BY name ASC OFFSET $1 LIMIT $2",
    )
    .bind(offset)
    .bind(limit)
    .fetch_all(pool.get_ref())
    .await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recipes")
        .fetch_one(pool.get_ref())
        .await?;
    log_info!(MODULE, "list", "user={} count={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok().json(Paginated::new(rows, params, total)))
}

#[get("/{id}/steps")]
pub async fn list_recipe_steps(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let rows = sqlx::query_as::<_, RecipeStep>(
        "SELECT * FROM recipe_steps WHERE recipe_id = $1 ORDER BY step_order ASC",
    )
    .bind(id)
    .fetch_all(pool.get_ref())
    .await?;
    log_info!(MODULE, "steps", "user={} recipe={} count={}", user.user_id(), id, rows.len());
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

// -----------------------------------------------------------------------------
// Step timers — read (anyone authed). Multi-concurrent timers per step per
// PRD §3; the frontend binds one TimerRing per row.
// -----------------------------------------------------------------------------
#[get("/{id}/timers")]
pub async fn list_step_timers(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let step_id = path.into_inner();
    let rows = sqlx::query_as::<_, StepTimer>(
        "SELECT id, step_id, label, duration_seconds, alert_type
         FROM step_timers WHERE step_id = $1
         ORDER BY label ASC",
    )
    .bind(step_id)
    .fetch_all(pool.get_ref())
    .await?;
    log_info!(MODULE, "step_timers_list", "user={} step={} count={}", user.user_id(), step_id, rows.len());
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

// -----------------------------------------------------------------------------
// Tip cards — read (anyone authed), write (ADMIN only)
// -----------------------------------------------------------------------------
#[get("/{id}/tip-cards")]
pub async fn list_tip_cards(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let step_id = path.into_inner();
    let rows = sqlx::query_as::<_, TipCard>(
        "SELECT * FROM tip_cards WHERE step_id = $1 ORDER BY is_pinned DESC, created_at ASC",
    )
    .bind(step_id)
    .fetch_all(pool.get_ref())
    .await?;
    log_info!(MODULE, "tip_cards_list", "user={} step={} count={}", user.user_id(), step_id, rows.len());
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

#[post("")]
pub async fn create_tip_card(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    body: web::Json<CreateTipCard>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let req = body.into_inner();
    if req.title.trim().is_empty() || req.content.trim().is_empty() {
        return Err(ApiError::BadRequest("title and content required".into()));
    }
    let row = sqlx::query_as::<_, TipCard>(
        "INSERT INTO tip_cards (step_id, title, content, authored_by, is_pinned)
         VALUES ($1, $2, $3, $4, COALESCE($5, TRUE))
         RETURNING *",
    )
    .bind(req.step_id)
    .bind(&req.title)
    .bind(&req.content)
    .bind(user.user_id())
    .bind(req.is_pinned)
    .fetch_one(pool.get_ref())
    .await?;
    log_info!(MODULE, "tip_cards_create", "user={} step={} card={}", user.user_id(), req.step_id, row.id);
    Ok(HttpResponse::Created().json(row))
}

#[put("/{id}")]
pub async fn update_tip_card(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdateTipCard>,
) -> Result<HttpResponse, ApiError> {
    require_role(&user, Role::Admin)?;
    let id = path.into_inner();
    let req = body.into_inner();
    let row = sqlx::query_as::<_, TipCard>(
        "UPDATE tip_cards
         SET title     = COALESCE($1, title),
             content   = COALESCE($2, content),
             is_pinned = COALESCE($3, is_pinned),
             updated_at = NOW()
         WHERE id = $4
         RETURNING *",
    )
    .bind(&req.title)
    .bind(&req.content)
    .bind(req.is_pinned)
    .bind(id)
    .fetch_optional(pool.get_ref())
    .await?;
    let row = row.ok_or_else(|| ApiError::NotFound("tip card not found".into()))?;
    log_info!(MODULE, "tip_cards_update", "user={} card={}", user.user_id(), id);
    Ok(HttpResponse::Ok().json(row))
}

// -----------------------------------------------------------------------------
// Scope wiring — three scopes because the API paths span three roots.
// -----------------------------------------------------------------------------
pub fn recipes_scope() -> actix_web::Scope {
    web::scope("/api/recipes")
        .service(list_recipes)
        .service(list_recipe_steps)
}

pub fn steps_scope() -> actix_web::Scope {
    web::scope("/api/steps")
        .service(list_tip_cards)
        .service(list_step_timers)
}

pub fn tip_cards_scope() -> actix_web::Scope {
    web::scope("/api/tip-cards")
        .service(create_tip_card)
        .service(update_tip_card)
}
