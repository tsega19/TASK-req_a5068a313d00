//! Auth HTTP endpoints: POST /api/auth/login, POST /api/auth/logout.
//!
//! Logout semantics: the JWT is stateless, so the server cannot invalidate a
//! token — the client MUST drop the token locally. The endpoint requires a
//! valid bearer (so the log line attributes the action to a user) and returns
//! `{"ok": true, "revoked": false}` to make the lack of server-side revocation
//! explicit to API consumers. A future revocation list would flip `revoked` to
//! `true` without changing callers.

use actix_web::{post, web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::auth::hashing::{hash_password, verify_password};
use crate::auth::jwt;
use crate::auth::models::{Role, UserRow};
use crate::config::AppConfig;
use crate::errors::ApiError;
use crate::middleware::rbac::AuthedUser;
use crate::{log_info, log_warn};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: PublicUser,
    /// Set when the user is still on a seeded/placeholder password and must
    /// rotate before issuing privileged requests. The frontend is expected
    /// to route the user to a "change password" step.
    pub password_reset_required: bool,
}

#[derive(Debug, Serialize)]
pub struct PublicUser {
    pub id: uuid::Uuid,
    pub username: String,
    pub role: Role,
    pub branch_id: Option<uuid::Uuid>,
    pub full_name: Option<String>,
}

#[post("/login")]
pub async fn login(
    body: web::Json<LoginRequest>,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
) -> Result<HttpResponse, ApiError> {
    let req = body.into_inner();
    if req.username.is_empty() || req.password.is_empty() {
        return Err(ApiError::BadRequest("username and password required".into()));
    }

    let row: Option<UserRow> = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, branch_id, full_name, privacy_mode,
                password_reset_required
         FROM users WHERE username = $1 AND deleted_at IS NULL",
    )
    .bind(&req.username)
    .fetch_optional(pool.get_ref())
    .await?;

    let user = match row {
        Some(u) => u,
        None => {
            log_warn!("auth", "login", "unknown username '{}'", req.username);
            return Err(ApiError::Unauthorized("invalid credentials".into()));
        }
    };

    if !verify_password(&req.password, &user.password_hash)? {
        log_warn!("auth", "login", "bad password for '{}'", req.username);
        return Err(ApiError::Unauthorized("invalid credentials".into()));
    }

    let token = jwt::issue(user.id, &user.username, user.role, user.branch_id, &cfg.auth)?;
    log_info!("auth", "login", "user '{}' authenticated", user.username);

    Ok(HttpResponse::Ok().json(LoginResponse {
        token,
        user: PublicUser {
            id: user.id,
            username: user.username,
            role: user.role,
            branch_id: user.branch_id,
            full_name: user.full_name,
        },
        password_reset_required: user.password_reset_required,
    }))
}

#[post("/logout")]
pub async fn logout(user: AuthedUser) -> Result<HttpResponse, ApiError> {
    log_info!("auth", "logout", "logout acknowledged user={} (stateless JWT — client must drop token)", user.user_id());
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "revoked": false,
        "note": "JWT is stateless; client must drop the token locally"
    })))
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[post("/change-password")]
pub async fn change_password(
    user: AuthedUser,
    body: web::Json<ChangePasswordRequest>,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
) -> Result<HttpResponse, ApiError> {
    let req = body.into_inner();
    if req.new_password.len() < 12 {
        return Err(ApiError::BadRequest(
            "new password must be at least 12 characters".into(),
        ));
    }
    let current_hash: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1 AND deleted_at IS NULL")
            .bind(user.user_id())
            .fetch_one(pool.get_ref())
            .await?;
    if !verify_password(&req.current_password, &current_hash)? {
        log_warn!("auth", "change_password", "bad current password user={}", user.user_id());
        return Err(ApiError::Unauthorized("current password incorrect".into()));
    }
    if verify_password(&req.new_password, &current_hash)? {
        return Err(ApiError::BadRequest(
            "new password must differ from current password".into(),
        ));
    }
    let new_hash = hash_password(&req.new_password, &cfg.auth)?;
    sqlx::query(
        "UPDATE users
         SET password_hash = $1,
             password_reset_required = FALSE,
             updated_at = NOW()
         WHERE id = $2",
    )
    .bind(&new_hash)
    .bind(user.user_id())
    .execute(pool.get_ref())
    .await?;
    log_info!("auth", "change_password", "user={} rotated password", user.user_id());
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

pub fn scope() -> actix_web::Scope {
    web::scope("/api/auth")
        .service(login)
        .service(logout)
        .service(change_password)
}
