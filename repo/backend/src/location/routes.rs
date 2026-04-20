//! Location-trail + check-in routes.
//!
//! Visibility (PRD §9):
//!   - ADMIN: full trail
//!   - TECH own + privacy OFF: full trail
//!   - SUPER: masked (and HIDDEN entirely when the owning tech has privacy_mode=true)
//!   - Others: none (NotFound to avoid enumeration)

use actix_web::{get, post, web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::models::Role;
use crate::enums::CheckInType;
use crate::errors::ApiError;
use crate::geo::{haversine_miles, reduce_precision};
use crate::location::geocode_stub;
use crate::middleware::rbac::{require_any_role, AuthedUser};
use crate::processing_log;
use crate::work_orders::routes::load_visible;
use crate::{log_info, log_warn};

const MODULE: &str = "location";

#[derive(Debug, Deserialize)]
pub struct TrailPoint {
    pub lat: f64,
    pub lng: f64,
}

#[derive(Debug, Deserialize)]
pub struct CheckInBody {
    #[serde(rename = "type")]
    pub kind: CheckInType,
    pub lat: f64,
    pub lng: f64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TrailRow {
    pub id: Uuid,
    pub work_order_id: Uuid,
    pub user_id: Uuid,
    pub lat: f64,
    pub lng: f64,
    pub precision_reduced: bool,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CheckInRow {
    pub id: Uuid,
    pub work_order_id: Uuid,
    pub user_id: Uuid,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub kind: CheckInType,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub recorded_at: DateTime<Utc>,
}

#[post("/{id}/location-trail")]
pub async fn post_trail_point(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<TrailPoint>,
) -> Result<HttpResponse, ApiError> {
    let wo_id = path.into_inner();
    let wo = load_visible(&pool, &user, wo_id).await?;
    // Trail integrity (PRD §9): only the assigned TECH records their own
    // trajectory. SUPER/ADMIN can read but MUST NOT inject points.
    if !matches!(user.role(), Role::Tech) || wo.assigned_tech_id != Some(user.user_id()) {
        return Err(ApiError::Forbidden(
            "only the assigned technician may record trail points".into(),
        ));
    }

    // Fetch owner's privacy flag.
    let privacy: Option<bool> =
        sqlx::query_scalar("SELECT privacy_mode FROM users WHERE id = $1")
            .bind(user.user_id())
            .fetch_optional(pool.get_ref())
            .await?;
    let privacy_on = privacy.unwrap_or(false);

    let (lat, lng, reduced) = if privacy_on {
        let (rlat, rlng) = reduce_precision(body.lat, body.lng);
        (rlat, rlng, true)
    } else {
        (body.lat, body.lng, false)
    };

    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, TrailRow>(
        "INSERT INTO location_trails
            (work_order_id, user_id, lat, lng, precision_reduced, recorded_at)
         VALUES ($1, $2, $3, $4, $5, NOW())
         RETURNING *",
    )
    .bind(wo_id)
    .bind(user.user_id())
    .bind(lat)
    .bind(lng)
    .bind(reduced)
    .fetch_one(&mut *tx)
    .await?;

    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::TRAIL_POINT,
        "location_trails",
        Some(row.id),
        json!({
            "work_order_id": wo_id,
            "precision_reduced": reduced,
        }),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "trail_post", "user={} wo={} reduced={}", user.user_id(), wo_id, reduced);
    Ok(HttpResponse::Created().json(row))
}

#[get("/{id}/location-trail")]
pub async fn get_trail(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let wo_id = path.into_inner();
    let wo = load_visible(&pool, &user, wo_id).await?;

    // Owner privacy state determines visibility for non-admin readers.
    let owner_privacy: Option<bool> = if let Some(tech) = wo.assigned_tech_id {
        sqlx::query_scalar("SELECT privacy_mode FROM users WHERE id = $1")
            .bind(tech)
            .fetch_optional(pool.get_ref())
            .await?
    } else {
        Some(false)
    };
    let owner_privacy = owner_privacy.unwrap_or(false);

    let (show_full, hidden): (bool, bool) = match user.role() {
        Role::Admin => (true, false),
        // Owner TECH always sees their own trail at full precision, regardless
        // of privacy_mode — privacy hides from OTHERS, not from the owner.
        Role::Tech if wo.assigned_tech_id == Some(user.user_id()) => (true, false),
        Role::Super => {
            if owner_privacy {
                (false, true) // HIDDEN from SUPER when privacy mode is on
            } else {
                (false, false) // masked — precision reduced on the way out
            }
        }
        _ => (false, true),
    };

    if hidden {
        log_warn!(MODULE, "trail_get", "user={} wo={} hidden by privacy", user.user_id(), wo_id);
        return Ok(HttpResponse::Ok().json(json!({ "data": [], "total": 0, "hidden": true })));
    }

    let mut rows = sqlx::query_as::<_, TrailRow>(
        "SELECT * FROM location_trails WHERE work_order_id = $1 ORDER BY recorded_at ASC",
    )
    .bind(wo_id)
    .fetch_all(pool.get_ref())
    .await?;

    if !show_full {
        for r in rows.iter_mut() {
            let (rlat, rlng) = reduce_precision(r.lat, r.lng);
            r.lat = rlat;
            r.lng = rlng;
            r.precision_reduced = true;
        }
    }

    log_info!(
        MODULE,
        "trail_get",
        "user={} wo={} count={} full={}",
        user.user_id(),
        wo_id,
        rows.len(),
        show_full
    );
    Ok(HttpResponse::Ok().json(json!({ "data": rows, "total": rows.len() })))
}

#[post("/{id}/check-in")]
pub async fn post_check_in(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
    body: web::Json<CheckInBody>,
) -> Result<HttpResponse, ApiError> {
    let wo_id = path.into_inner();
    let wo = load_visible(&pool, &user, wo_id).await?;
    if matches!(user.role(), Role::Tech) && wo.assigned_tech_id != Some(user.user_id()) {
        return Err(ApiError::Forbidden("not assigned to this work order".into()));
    }

    // Radius validation for ARRIVAL only (PRD §7).
    if body.kind == CheckInType::Arrival {
        if let Some(branch_id) = wo.branch_id {
            let branch: Option<(Option<f64>, Option<f64>, i32)> = sqlx::query_as(
                "SELECT lat, lng, service_radius_miles FROM branches WHERE id = $1",
            )
            .bind(branch_id)
            .fetch_optional(pool.get_ref())
            .await?;
            if let Some((Some(blat), Some(blng), radius)) = branch {
                let d = haversine_miles(body.lat, body.lng, blat, blng);
                if d > radius as f64 {
                    return Err(ApiError::BadRequest(format!(
                        "arrival check-in {:.1}mi from branch — exceeds {}mi radius",
                        d, radius
                    )));
                }
            }
        }
    }

    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, CheckInRow>(
        "INSERT INTO check_ins (work_order_id, user_id, type, lat, lng, recorded_at)
         VALUES ($1, $2, $3, $4, $5, NOW())
         RETURNING id, work_order_id, user_id, type, lat, lng, recorded_at",
    )
    .bind(wo_id)
    .bind(user.user_id())
    .bind(body.kind)
    .bind(body.lat)
    .bind(body.lng)
    .fetch_one(&mut *tx)
    .await?;

    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::CHECK_IN,
        "check_ins",
        Some(row.id),
        json!({
            "work_order_id": wo_id,
            "type": format!("{:?}", body.kind),
        }),
    )
    .await?;
    tx.commit().await?;
    log_info!(
        MODULE,
        "check_in",
        "user={} wo={} type={:?}",
        user.user_id(),
        wo_id,
        body.kind
    );
    Ok(HttpResponse::Created().json(row))
}

/// Dedicated offline-geocoding endpoint (PRD §6). Supervisors and admins can
/// pre-resolve an address to its canonical ZIP+4 form and coordinates before
/// creating a work order, so the UI shows the normalized value up front.
#[derive(Debug, Deserialize)]
pub struct GeocodeRequest {
    pub query: Option<String>,
    pub zip4: Option<String>,
    pub street: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GeocodeResponse {
    pub address_norm: String,
    pub lat: f64,
    pub lng: f64,
    pub from_index: bool,
}

#[post("/geocode")]
pub async fn geocode(
    user: AuthedUser,
    cfg: web::Data<crate::config::AppConfig>,
    body: web::Json<GeocodeRequest>,
) -> Result<HttpResponse, ApiError> {
    require_any_role(&user, &[Role::Super, Role::Admin])?;
    let req = body.into_inner();
    let result = if let Some(z) = req.zip4.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        match geocode_stub::normalize(z, req.street.as_deref()) {
            Some(g) => g,
            None => {
                return Err(ApiError::NotFound(format!(
                    "zip4 '{}' not in bundled index",
                    z
                )))
            }
        }
    } else if let Some(q) = req.query.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let g = geocode_stub::geocode(q);
        // Deny the synthetic-fallback result in strict mode so unknown
        // addresses don't silently produce synthetic coordinates that then
        // taint radius checks and trail analytics.
        if !g.from_index && !cfg.app.allow_geocode_fallback {
            return Err(ApiError::BadRequest(format!(
                "address '{}' not found in bundled ZIP+4 index",
                q
            )));
        }
        g
    } else {
        return Err(ApiError::BadRequest(
            "provide either zip4 (+optional street) or query".into(),
        ));
    };
    log_info!(
        MODULE,
        "geocode",
        "user={} from_index={}",
        user.user_id(),
        result.from_index
    );
    Ok(HttpResponse::Ok().json(GeocodeResponse {
        address_norm: result.address_norm,
        lat: result.lat,
        lng: result.lng,
        from_index: result.from_index,
    }))
}
