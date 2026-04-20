//! Integration-test harness. Each test calls `setup().await` to get a fresh
//! database (truncated + seeded) plus a ready-to-use actix-web test service.

use actix_web::body::{BoxBody, EitherBody, MessageBody};
use actix_web::{dev::ServiceResponse, test, web, App};
use fieldops_backend::auth::hashing::hash_password;
use fieldops_backend::auth::jwt;
use fieldops_backend::auth::models::Role;
use fieldops_backend::config::AppConfig;
use fieldops_backend::middleware::rbac::JwtAuth;
use fieldops_backend::{configure, db};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Per-test context.
#[derive(Clone)]
pub struct Ctx {
    pub pool: PgPool,
    pub cfg: AppConfig,
    pub admin_id: Uuid,
    pub super_id: Uuid,
    pub tech_a_id: Uuid,
    pub tech_b_id: Uuid,
    pub branch_a_id: Uuid,
    pub branch_b_id: Uuid,
    pub recipe_id: Uuid,
    pub step_ids: Vec<Uuid>,
    pub wo_a_id: Uuid,
    pub wo_b_id: Uuid,
    pub admin_token: String,
    pub super_token: String,
    pub tech_a_token: String,
    pub tech_b_token: String,
}

pub async fn setup() -> Ctx {
    let cfg = AppConfig::test();
    let pool = PgPool::connect(&cfg.database.url)
        .await
        .expect("connect to test postgres");
    db::run_migrations(&pool).await.expect("run migrations");
    db::truncate_all(&pool).await.expect("truncate");

    // Branches
    let branch_a_id: Uuid = sqlx::query_scalar(
        "INSERT INTO branches (name, address, lat, lng, service_radius_miles)
         VALUES ('Branch A', '123 Main', 37.7749, -122.4194, 30) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let branch_b_id: Uuid = sqlx::query_scalar(
        "INSERT INTO branches (name, address, lat, lng, service_radius_miles)
         VALUES ('Branch B', '456 Elm',  40.7128, -74.0060, 30) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Users (all share password 'pw' for tests)
    let hash = hash_password("pw", &cfg.auth).unwrap();
    let admin_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, full_name)
         VALUES ('admin', $1, 'ADMIN', 'Admin User') RETURNING id",
    )
    .bind(&hash)
    .fetch_one(&pool)
    .await
    .unwrap();
    let super_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, branch_id, full_name)
         VALUES ('super_a', $1, 'SUPER', $2, 'Super A') RETURNING id",
    )
    .bind(&hash)
    .bind(branch_a_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let tech_a_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, branch_id, full_name)
         VALUES ('tech_a', $1, 'TECH', $2, 'Tech A') RETURNING id",
    )
    .bind(&hash)
    .bind(branch_a_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let tech_b_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, role, branch_id, full_name)
         VALUES ('tech_b', $1, 'TECH', $2, 'Tech B') RETURNING id",
    )
    .bind(&hash)
    .bind(branch_b_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Recipe + steps
    let recipe_id: Uuid = sqlx::query_scalar(
        "INSERT INTO recipes (name, description, created_by)
         VALUES ('Refrigeration Service', 'Standard service', $1) RETURNING id",
    )
    .bind(admin_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut step_ids = Vec::new();
    for (order, title) in &[(1, "Prep"), (2, "Service"), (3, "Verify")] {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO recipe_steps (recipe_id, step_order, title, instructions)
             VALUES ($1, $2, $3, 'Follow procedure.') RETURNING id",
        )
        .bind(recipe_id)
        .bind(*order)
        .bind(*title)
        .fetch_one(&pool)
        .await
        .unwrap();
        step_ids.push(id);
    }

    // Work orders
    let wo_a_id: Uuid = sqlx::query_scalar(
        "INSERT INTO work_orders
            (title, description, priority, state, assigned_tech_id, branch_id,
             sla_deadline, recipe_id, location_address_norm, location_lat, location_lng,
             version_count)
         VALUES ('WO-A', 'Primary', 'HIGH', 'Scheduled', $1, $2,
                 NOW() + INTERVAL '8 hours', $3, '123 Main', 37.7749, -122.4194, 1)
         RETURNING id",
    )
    .bind(tech_a_id)
    .bind(branch_a_id)
    .bind(recipe_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let wo_b_id: Uuid = sqlx::query_scalar(
        "INSERT INTO work_orders
            (title, description, priority, state, assigned_tech_id, branch_id,
             sla_deadline, recipe_id, location_address_norm, location_lat, location_lng,
             version_count)
         VALUES ('WO-B', 'Secondary', 'NORMAL', 'Scheduled', $1, $2,
                 NOW() + INTERVAL '24 hours', $3, '456 Elm', 40.7128, -74.0060, 1)
         RETURNING id",
    )
    .bind(tech_b_id)
    .bind(branch_b_id)
    .bind(recipe_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let admin_token =
        jwt::issue(admin_id, "admin", Role::Admin, None, &cfg.auth).unwrap();
    let super_token = jwt::issue(super_id, "super_a", Role::Super, Some(branch_a_id), &cfg.auth)
        .unwrap();
    let tech_a_token = jwt::issue(tech_a_id, "tech_a", Role::Tech, Some(branch_a_id), &cfg.auth)
        .unwrap();
    let tech_b_token = jwt::issue(tech_b_id, "tech_b", Role::Tech, Some(branch_b_id), &cfg.auth)
        .unwrap();

    Ctx {
        pool,
        cfg,
        admin_id,
        super_id,
        tech_a_id,
        tech_b_id,
        branch_a_id,
        branch_b_id,
        recipe_id,
        step_ids,
        wo_a_id,
        wo_b_id,
        admin_token,
        super_token,
        tech_a_token,
        tech_b_token,
    }
}

/// Build the actix-web test service for the given context.
///
/// The `JwtAuth` middleware wraps responses in `EitherBody<BoxBody>`, so that
/// shows up explicitly in the return type — the helpers below are generic over
/// the body so tests don't need to care.
pub async fn make_service(
    ctx: &Ctx,
) -> impl actix_web::dev::Service<
    actix_http::Request,
    Response = ServiceResponse<EitherBody<BoxBody>>,
    Error = actix_web::Error,
> {
    let cfg = ctx.cfg.clone();
    let pool = ctx.pool.clone();
    test::init_service(
        App::new()
            .app_data(web::Data::new(cfg))
            .app_data(web::Data::new(pool))
            .wrap(JwtAuth)
            .configure(configure),
    )
    .await
}

/// Variant of `make_service` that accepts a caller-supplied `AppConfig`.
/// Used by strict-mode tests that need to flip feature flags
/// (e.g. `allow_geocode_fallback`) without polluting the default `Ctx`.
pub async fn make_service_with_cfg(
    ctx: &Ctx,
    cfg: AppConfig,
) -> impl actix_web::dev::Service<
    actix_http::Request,
    Response = ServiceResponse<EitherBody<BoxBody>>,
    Error = actix_web::Error,
> {
    let pool = ctx.pool.clone();
    test::init_service(
        App::new()
            .app_data(web::Data::new(cfg))
            .app_data(web::Data::new(pool))
            .wrap(JwtAuth)
            .configure(configure),
    )
    .await
}

pub async fn status_of<S, B>(svc: &S, req: actix_http::Request) -> u16
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse<B>,
        Error = actix_web::Error,
    >,
    B: MessageBody,
{
    let resp = test::call_service(svc, req).await;
    resp.status().as_u16()
}

pub async fn json_of<S, T, B>(svc: &S, req: actix_http::Request) -> (u16, T)
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse<B>,
        Error = actix_web::Error,
    >,
    B: MessageBody,
    T: DeserializeOwned,
{
    let resp = test::call_service(svc, req).await;
    let status = resp.status().as_u16();
    let body: T = test::read_body_json(resp).await;
    (status, body)
}

pub async fn raw_of<S, B>(svc: &S, req: actix_http::Request) -> (u16, Value)
where
    S: actix_web::dev::Service<
        actix_http::Request,
        Response = ServiceResponse<B>,
        Error = actix_web::Error,
    >,
    B: MessageBody,
{
    let resp = test::call_service(svc, req).await;
    let status = resp.status().as_u16();
    let body = test::read_body(resp).await;
    let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

pub fn auth_header(token: &str) -> (&'static str, String) {
    ("Authorization", format!("Bearer {}", token))
}
