//! Self-service endpoints for the authenticated caller.
//! Currently scoped to the privacy toggle required by the Map View.

use actix_web::{get, put, web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::auth::models::{Role, UserRow};
use crate::config::AppConfig;
use crate::crypto;
use crate::errors::ApiError;
use crate::middleware::rbac::AuthedUser;
use crate::processing_log;
use crate::{log_info, log_warn};

const MODULE: &str = "me";

#[derive(Debug, Deserialize)]
pub struct PrivacyBody {
    pub privacy_mode: bool,
}

#[derive(Debug, Serialize)]
pub struct PublicProfile {
    pub id: uuid::Uuid,
    pub username: String,
    pub role: Role,
    pub branch_id: Option<uuid::Uuid>,
    pub full_name: Option<String>,
    pub privacy_mode: bool,
}

#[derive(Debug, Deserialize)]
pub struct HomeAddressBody {
    pub home_address: String,
}

#[derive(Debug, Serialize)]
pub struct HomeAddressResponse {
    /// Plaintext is only returned to the authenticated owner — never logged,
    /// never embedded in error strings.
    pub home_address: Option<String>,
    pub stored: bool,
}

#[get("")]
pub async fn get_me(
    user: AuthedUser,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let row: UserRow = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, role, branch_id, full_name, privacy_mode,
                password_reset_required
         FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(user.user_id())
    .fetch_one(pool.get_ref())
    .await?;
    Ok(HttpResponse::Ok().json(PublicProfile {
        id: row.id,
        username: row.username,
        role: row.role,
        branch_id: row.branch_id,
        full_name: row.full_name,
        privacy_mode: row.privacy_mode,
    }))
}

#[put("/privacy")]
pub async fn set_privacy(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    body: web::Json<PrivacyBody>,
) -> Result<HttpResponse, ApiError> {
    let new_val = body.privacy_mode;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE users SET privacy_mode = $1, updated_at = NOW()
         WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(new_val)
    .bind(user.user_id())
    .execute(&mut *tx)
    .await?;
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::ME_PRIVACY_SET,
        "users",
        Some(user.user_id()),
        serde_json::json!({ "privacy_mode": new_val }),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "set_privacy", "user={} privacy_mode={}", user.user_id(), new_val);
    Ok(HttpResponse::Ok().json(serde_json::json!({ "privacy_mode": new_val })))
}

#[put("/home-address")]
pub async fn set_home_address(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
    body: web::Json<HomeAddressBody>,
) -> Result<HttpResponse, ApiError> {
    let plaintext = body.into_inner().home_address;
    if plaintext.trim().is_empty() {
        return Err(ApiError::BadRequest("home_address required".into()));
    }
    let ciphertext = crypto::encrypt(&plaintext, &cfg.encryption.aes_256_key)?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE users SET home_address_enc = $1, updated_at = NOW()
         WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(&ciphertext)
    .bind(user.user_id())
    .execute(&mut *tx)
    .await?;
    // NOTE: never persist plaintext in the audit row — only a length signal so
    // the trail shows *that* an address was written without leaking its value.
    processing_log::record_tx(
        &mut tx,
        Some(user.user_id()),
        processing_log::actions::ME_HOME_ADDRESS_SET,
        "users",
        Some(user.user_id()),
        serde_json::json!({ "length": plaintext.len() }),
    )
    .await?;
    tx.commit().await?;
    log_info!(MODULE, "home_address_set", "user={} len={}", user.user_id(), plaintext.len());
    Ok(HttpResponse::Ok().json(HomeAddressResponse {
        home_address: Some(plaintext),
        stored: true,
    }))
}

#[get("/home-address")]
pub async fn get_home_address(
    user: AuthedUser,
    pool: web::Data<PgPool>,
    cfg: web::Data<AppConfig>,
) -> Result<HttpResponse, ApiError> {
    let enc: Option<String> = sqlx::query_scalar(
        "SELECT home_address_enc FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(user.user_id())
    .fetch_one(pool.get_ref())
    .await?;
    let plaintext = match enc {
        Some(ct) => match crypto::decrypt(&ct, &cfg.encryption.aes_256_key) {
            Ok(pt) => Some(pt),
            Err(e) => {
                // Don't leak ciphertext; this means the stored value was
                // written with a different key (key rotation migration needed).
                log_warn!(MODULE, "home_address_decrypt_failed", "user={} {}", user.user_id(), e);
                return Err(ApiError::Internal(
                    "stored address could not be decrypted".into(),
                ));
            }
        },
        None => None,
    };
    log_info!(MODULE, "home_address_get", "user={} present={}", user.user_id(), plaintext.is_some());
    Ok(HttpResponse::Ok().json(HomeAddressResponse {
        stored: plaintext.is_some(),
        home_address: plaintext,
    }))
}

pub fn scope() -> actix_web::Scope {
    web::scope("/api/me")
        .service(get_me)
        .service(set_privacy)
        .service(set_home_address)
        .service(get_home_address)
}
