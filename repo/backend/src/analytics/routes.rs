//! Learning analytics + watermarked CSV export.
//!
//! Date filters are MM/DD/YYYY per PRD §7.
//! Role scope (PRD §9):
//!   - TECH: own records only
//!   - SUPER: team (users.branch_id = caller's branch)
//!   - ADMIN: all

use actix_web::{get, web, HttpResponse};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::models::Role;
use crate::errors::ApiError;
use crate::middleware::rbac::AuthedUser;
use crate::log_info;

const MODULE: &str = "analytics";

#[derive(Debug, Deserialize)]
pub struct LearningQuery {
    pub from: Option<String>,    // MM/DD/YYYY
    pub to: Option<String>,      // MM/DD/YYYY
    pub branch: Option<Uuid>,
    pub role: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LearningRow {
    pub user_id: Uuid,
    pub username: String,
    pub role: crate::auth::models::Role,
    pub branch_id: Option<Uuid>,
    pub quiz_avg: Option<f64>,
    pub time_spent_total: Option<i64>,
    pub completion_count: Option<i64>,
    pub review_total: Option<i64>,
}

fn parse_mmddyyyy(s: &str) -> Result<DateTime<Utc>, ApiError> {
    NaiveDate::parse_from_str(s, "%m/%d/%Y")
        .map_err(|_| ApiError::BadRequest(format!("invalid date '{}' (expected MM/DD/YYYY)", s)))
        .and_then(|d| {
            d.and_hms_opt(0, 0, 0)
                .ok_or_else(|| ApiError::BadRequest("invalid date".into()))
                .map(|ndt| Utc.from_utc_datetime(&ndt))
        })
}

async fn query_learning(
    pool: &PgPool,
    user: &AuthedUser,
    q: &LearningQuery,
) -> Result<Vec<LearningRow>, ApiError> {
    let from = q.from.as_deref().map(parse_mmddyyyy).transpose()?;
    let to = q.to.as_deref().map(parse_mmddyyyy).transpose()?;
    let role_filter = q
        .role
        .as_deref()
        .map(|s| s.parse::<Role>().map_err(ApiError::BadRequest))
        .transpose()?;

    // Scope per caller role.
    let (scope_user, scope_branch): (Option<Uuid>, Option<Uuid>) = match user.role() {
        Role::Tech => (Some(user.user_id()), None),
        Role::Super => (None, user.branch_id()),
        Role::Admin => (None, None),
    };
    // Query-supplied branch filter narrows further, but never widens.
    let effective_branch = match (scope_branch, q.branch) {
        (Some(b), Some(req)) if b == req => Some(b),
        (Some(b), Some(_)) => Some(b), // ignore mismatched override
        (Some(b), None) => Some(b),
        (None, b) => b,
    };

    let rows = sqlx::query_as::<_, LearningRow>(
        "SELECT u.id AS user_id,
                u.username,
                u.role,
                u.branch_id,
                AVG(lr.quiz_score) AS quiz_avg,
                SUM(lr.time_spent_seconds)::BIGINT AS time_spent_total,
                COUNT(lr.completed_at)::BIGINT AS completion_count,
                SUM(lr.review_count)::BIGINT AS review_total
         FROM users u
         LEFT JOIN learning_records lr ON lr.user_id = u.id
            AND ($1::timestamptz IS NULL OR lr.completed_at >= $1)
            AND ($2::timestamptz IS NULL OR lr.completed_at <  $2 + INTERVAL '1 day')
         WHERE u.deleted_at IS NULL
           AND ($3::uuid IS NULL OR u.id = $3)
           AND ($4::uuid IS NULL OR u.branch_id = $4)
           AND ($5::user_role IS NULL OR u.role = $5)
         GROUP BY u.id, u.username, u.role, u.branch_id
         ORDER BY u.username ASC",
    )
    .bind(from)
    .bind(to)
    .bind(scope_user)
    .bind(effective_branch)
    .bind(role_filter)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

#[get("/learning")]
pub async fn learning(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<LearningQuery>,
) -> Result<HttpResponse, ApiError> {
    let q = q.into_inner();
    let rows = query_learning(&pool, &user, &q).await?;
    log_info!(MODULE, "learning", "user={} rows={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok()
        .json(serde_json::json!({ "data": rows, "total": rows.len() })))
}

#[get("/learning/export-csv")]
pub async fn learning_csv(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<LearningQuery>,
) -> Result<HttpResponse, ApiError> {
    let q = q.into_inner();
    let rows = query_learning(&pool, &user, &q).await?;

    let mut out = Vec::<u8>::new();
    {
        let mut w = csv::Writer::from_writer(&mut out);
        w.write_record([
            "user_id",
            "username",
            "role",
            "branch_id",
            "quiz_avg",
            "time_spent_total",
            "completion_count",
            "review_total",
        ])
        .map_err(|e| ApiError::Internal(e.to_string()))?;
        for r in &rows {
            w.write_record([
                r.user_id.to_string(),
                r.username.clone(),
                r.role.to_string(),
                r.branch_id.map(|b| b.to_string()).unwrap_or_default(),
                r.quiz_avg.map(|v| format!("{:.2}", v)).unwrap_or_default(),
                r.time_spent_total.map(|v| v.to_string()).unwrap_or_default(),
                r.completion_count.map(|v| v.to_string()).unwrap_or_default(),
                r.review_total.map(|v| v.to_string()).unwrap_or_default(),
            ])
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        }
        w.flush().map_err(|e| ApiError::Internal(e.to_string()))?;
    }

    // Watermark footer (PRD §7).
    let footer = format!(
        "\n# Exported by: {} at {}\n",
        user.0.username,
        Utc::now().to_rfc3339()
    );
    out.extend_from_slice(footer.as_bytes());

    log_info!(MODULE, "learning_csv", "user={} rows={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok()
        .content_type("text/csv; charset=utf-8")
        .insert_header((
            "Content-Disposition",
            "attachment; filename=\"learning-analytics.csv\"",
        ))
        .body(out))
}

pub fn scope() -> actix_web::Scope {
    web::scope("/api/analytics")
        .service(learning_csv) // more specific first
        .service(learning)
}
