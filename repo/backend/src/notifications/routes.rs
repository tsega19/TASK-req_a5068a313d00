//! Notification Center endpoints.
//! Every route is object-scoped: users may only see/modify their own rows.

use actix_web::{get, put, web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::enums::NotificationTemplate;
use crate::errors::ApiError;
use crate::middleware::rbac::AuthedUser;
use crate::pagination::{PageParams, Paginated};
use crate::log_info;

const MODULE: &str = "notifications";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NotificationRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub template_type: NotificationTemplate,
    pub payload: serde_json::Value,
    pub delivered_at: Option<DateTime<Utc>>,
    pub read_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
    pub is_unsubscribed: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UnsubscribeBody {
    pub template_type: NotificationTemplate,
}

#[get("")]
pub async fn list_notifications(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<PageParams>,
) -> Result<HttpResponse, ApiError> {
    let params = q.into_inner();
    let (offset, limit) = params.offset_limit();
    let rows = sqlx::query_as::<_, NotificationRow>(
        "SELECT * FROM notifications
         WHERE user_id = $1
         ORDER BY created_at DESC
         OFFSET $2 LIMIT $3",
    )
    .bind(user.user_id())
    .bind(offset)
    .bind(limit)
    .fetch_all(pool.get_ref())
    .await?;
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id = $1")
            .bind(user.user_id())
            .fetch_one(pool.get_ref())
            .await?;
    log_info!(MODULE, "list", "user={} count={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok().json(Paginated::new(rows, params, total)))
}

#[put("/{id}/read")]
pub async fn mark_read(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    let row = sqlx::query_as::<_, NotificationRow>(
        "UPDATE notifications SET read_at = NOW()
         WHERE id = $1 AND user_id = $2
         RETURNING *",
    )
    .bind(id)
    .bind(user.user_id())
    .fetch_optional(pool.get_ref())
    .await?;
    let row = row.ok_or_else(|| ApiError::NotFound("notification not found".into()))?;
    log_info!(MODULE, "mark_read", "user={} id={}", user.user_id(), id);
    Ok(HttpResponse::Ok().json(row))
}

#[put("/unsubscribe")]
pub async fn unsubscribe(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    body: web::Json<UnsubscribeBody>,
) -> Result<HttpResponse, ApiError> {
    let template = body.template_type;
    sqlx::query(
        "INSERT INTO notification_unsubscribes (user_id, template_type)
         VALUES ($1, $2)
         ON CONFLICT (user_id, template_type) DO NOTHING",
    )
    .bind(user.user_id())
    .bind(template)
    .execute(pool.get_ref())
    .await?;
    log_info!(MODULE, "unsubscribe", "user={} template={:?}", user.user_id(), template);
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true, "template_type": template })))
}

pub fn scope() -> actix_web::Scope {
    web::scope("/api/notifications")
        .service(unsubscribe) // registered BEFORE /{id}/read to avoid path clash
        .service(list_notifications)
        .service(mark_read)
}
