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

// ---------------------------------------------------------------------------
// Trend endpoints (PRD analytics: supervisor insights by knowledge point /
// learning unit / workflow, over time). `completion_count` is the raw pass
// count; `completion_rate` is the share of records whose `quiz_score >= 1.0`
// — the stronger signal supervisors actually need.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TrendQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub branch: Option<Uuid>,
    /// Optional time bucket. Accepted values: `day`, `week`, `month`. When
    /// omitted the endpoint returns a single row per group (no time axis).
    pub bucket: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TrendRow {
    pub group_id: Option<Uuid>,
    pub group_label: Option<String>,
    pub bucket_start: Option<DateTime<Utc>>,
    pub attempt_count: Option<i64>,
    pub completion_count: Option<i64>,
    pub completion_rate: Option<f64>,
    pub avg_quiz_score: Option<f64>,
    pub avg_time_spent_seconds: Option<f64>,
}

/// Translate an optional `bucket` param into the `date_trunc` literal Postgres
/// accepts. Returns `None` when no bucketing should apply.
fn parse_bucket(b: Option<&str>) -> Result<Option<&'static str>, ApiError> {
    match b.map(|s| s.trim().to_ascii_lowercase()) {
        None => Ok(None),
        Some(ref s) if s.is_empty() => Ok(None),
        Some(ref s) if s == "day" => Ok(Some("day")),
        Some(ref s) if s == "week" => Ok(Some("week")),
        Some(ref s) if s == "month" => Ok(Some("month")),
        Some(other) => Err(ApiError::BadRequest(format!(
            "invalid bucket '{}' (expected day|week|month)",
            other
        ))),
    }
}

/// Shared scope resolver — mirrors `query_learning`. TECH is own-only;
/// SUPER is branch-scoped; ADMIN is global. Caller-supplied `branch` can
/// narrow but never widen.
fn resolve_scope(
    user: &AuthedUser,
    q_branch: Option<Uuid>,
) -> (Option<Uuid>, Option<Uuid>) {
    let (scope_user, scope_branch) = match user.role() {
        Role::Tech => (Some(user.user_id()), None),
        Role::Super => (None, user.branch_id()),
        Role::Admin => (None, None),
    };
    let effective_branch = match (scope_branch, q_branch) {
        (Some(b), _) => Some(b),
        (None, b) => b,
    };
    (scope_user, effective_branch)
}

enum TrendGroup {
    KnowledgePoint,
    Unit,     // = recipe
    Workflow, // = work_order
}

async fn query_trends(
    pool: &PgPool,
    user: &AuthedUser,
    group: TrendGroup,
    q: &TrendQuery,
) -> Result<Vec<TrendRow>, ApiError> {
    let from = q.from.as_deref().map(parse_mmddyyyy).transpose()?;
    let to = q.to.as_deref().map(parse_mmddyyyy).transpose()?;
    let bucket = parse_bucket(q.bucket.as_deref())?;
    let (scope_user, effective_branch) = resolve_scope(user, q.branch);

    // Each group mode selects a different "group_id" / "group_label" pair and
    // joins the tables needed to compute `completion_rate` per-row.
    let (group_id_expr, group_label_expr, extra_joins, group_extra) = match group {
        TrendGroup::KnowledgePoint => (
            "kp.id",
            "kp.title",
            "JOIN knowledge_points kp ON kp.id = lr.knowledge_point_id",
            "kp.id, kp.title",
        ),
        TrendGroup::Unit => (
            "r.id",
            "r.name",
            "JOIN knowledge_points kp ON kp.id = lr.knowledge_point_id \
             JOIN recipes r ON r.id = kp.recipe_id",
            "r.id, r.name",
        ),
        TrendGroup::Workflow => (
            "wo.id",
            "wo.title",
            "LEFT JOIN work_orders wo ON wo.id = lr.work_order_id",
            "wo.id, wo.title",
        ),
    };

    let (bucket_select, bucket_group_extra, bucket_order_by) = match bucket {
        Some(b) => (
            format!("date_trunc('{}', lr.completed_at)", b),
            format!(", date_trunc('{}', lr.completed_at)", b),
            ", bucket_start ASC NULLS LAST".to_string(),
        ),
        None => (
            "NULL::timestamptz".to_string(),
            String::new(),
            String::new(),
        ),
    };

    // `completion_rate` = share of graded records that passed (quiz_score >= 1.0)
    // within the group. FILTER drops rows without a quiz so reading-only KPs
    // don't drag the rate toward zero. `::DOUBLE PRECISION` casts on the
    // numeric aggregates so sqlx can decode them straight into `f64` fields
    // without pulling in the `NUMERIC` feature.
    let sql = format!(
        "SELECT {group_id} AS group_id,
                {group_label} AS group_label,
                {bucket_select} AS bucket_start,
                COUNT(*)::BIGINT AS attempt_count,
                SUM(CASE WHEN lr.quiz_score >= 1.0 THEN 1 ELSE 0 END)::BIGINT AS completion_count,
                AVG(CASE WHEN lr.quiz_score >= 1.0 THEN 1.0::DOUBLE PRECISION ELSE 0.0 END)
                    FILTER (WHERE lr.quiz_score IS NOT NULL) AS completion_rate,
                AVG(lr.quiz_score)::DOUBLE PRECISION AS avg_quiz_score,
                AVG(lr.time_spent_seconds)::DOUBLE PRECISION AS avg_time_spent_seconds
         FROM learning_records lr
         JOIN users u ON u.id = lr.user_id
         {extra_joins}
         WHERE lr.completed_at IS NOT NULL
           AND ($1::timestamptz IS NULL OR lr.completed_at >= $1)
           AND ($2::timestamptz IS NULL OR lr.completed_at <  $2 + INTERVAL '1 day')
           AND ($3::uuid IS NULL OR lr.user_id = $3)
           AND ($4::uuid IS NULL OR u.branch_id = $4)
         GROUP BY {group_extra}{bucket_group_extra}
         ORDER BY group_label ASC NULLS LAST{bucket_order_by}",
        group_id = group_id_expr,
        group_label = group_label_expr,
        bucket_select = bucket_select,
        extra_joins = extra_joins,
        group_extra = group_extra,
        bucket_group_extra = bucket_group_extra,
        bucket_order_by = bucket_order_by,
    );

    let rows = sqlx::query_as::<_, TrendRow>(&sql)
        .bind(from)
        .bind(to)
        .bind(scope_user)
        .bind(effective_branch)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

#[get("/trends/knowledge-points")]
pub async fn trends_knowledge_points(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<TrendQuery>,
) -> Result<HttpResponse, ApiError> {
    let q = q.into_inner();
    let rows = query_trends(&pool, &user, TrendGroup::KnowledgePoint, &q).await?;
    log_info!(MODULE, "trends_kp", "user={} rows={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": rows, "total": rows.len() })))
}

#[get("/trends/units")]
pub async fn trends_units(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<TrendQuery>,
) -> Result<HttpResponse, ApiError> {
    let q = q.into_inner();
    let rows = query_trends(&pool, &user, TrendGroup::Unit, &q).await?;
    log_info!(MODULE, "trends_unit", "user={} rows={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": rows, "total": rows.len() })))
}

#[get("/trends/workflows")]
pub async fn trends_workflows(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    q: web::Query<TrendQuery>,
) -> Result<HttpResponse, ApiError> {
    let q = q.into_inner();
    let rows = query_trends(&pool, &user, TrendGroup::Workflow, &q).await?;
    log_info!(MODULE, "trends_workflow", "user={} rows={}", user.user_id(), rows.len());
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": rows, "total": rows.len() })))
}

pub fn scope() -> actix_web::Scope {
    web::scope("/api/analytics")
        // Trend endpoints must be registered before `/learning` so the more
        // specific paths win when actix-web matches.
        .service(trends_knowledge_points)
        .service(trends_units)
        .service(trends_workflows)
        .service(learning_csv) // more specific first
        .service(learning)
}
