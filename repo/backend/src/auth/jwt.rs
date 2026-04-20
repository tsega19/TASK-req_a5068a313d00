//! JWT token issuance & validation. HS256 with the configured secret.
//!
//! `iss` and `aud` are both set on issue and enforced on verify. Binding
//! tokens to a specific deployment (`fieldops-backend` → `fieldops-frontend`
//! by default) means a token minted for one service cannot be replayed at
//! another — defense-in-depth against cross-deployment token confusion
//! beyond the signature/expiry checks.

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::models::Role;
use crate::config::AuthConfig;
use crate::errors::ApiError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub username: String,
    pub role: Role,
    pub branch_id: Option<Uuid>,
    pub exp: i64,
    pub iat: i64,
    /// Issuer — must match `AuthConfig::jwt_issuer` on verify.
    pub iss: String,
    /// Audience — must match `AuthConfig::jwt_audience` on verify.
    pub aud: String,
}

pub fn issue(
    user_id: Uuid,
    username: &str,
    role: Role,
    branch_id: Option<Uuid>,
    cfg: &AuthConfig,
) -> Result<String, ApiError> {
    let now = Utc::now();
    let exp = (now + Duration::hours(cfg.jwt_expiry_hours)).timestamp();
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        role,
        branch_id,
        exp,
        iat: now.timestamp(),
        iss: cfg.jwt_issuer.clone(),
        aud: cfg.jwt_audience.clone(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(cfg.jwt_secret.as_bytes()),
    )
    .map_err(|e| ApiError::Internal(format!("jwt encode: {}", e)))
}

pub fn verify(token: &str, cfg: &AuthConfig) -> Result<Claims, ApiError> {
    // `Validation::default()` only checks signature + expiry. We bind the
    // token to this deployment by setting `iss` and `aud` explicitly; the
    // jsonwebtoken crate then enforces both on decode.
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[cfg.jwt_issuer.as_str()]);
    validation.set_audience(&[cfg.jwt_audience.as_str()]);
    // `leeway` and `validate_exp` remain at their defaults (60s, true).

    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(cfg.jwt_secret.as_bytes()),
        &validation,
    )
    .map_err(|e| ApiError::Unauthorized(format!("invalid token: {}", e)))?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AuthConfig {
        AuthConfig {
            jwt_secret: "test-secret-xxxxxxxxxxxxxxxxxxxx".into(),
            jwt_expiry_hours: 1,
            jwt_issuer: "fieldops-test".into(),
            jwt_audience: "fieldops-test-aud".into(),
            argon2_memory_kib: 19456,
            argon2_iterations: 2,
            argon2_parallelism: 1,
        }
    }

    #[test]
    fn issues_and_verifies() {
        let c = cfg();
        let uid = Uuid::new_v4();
        let token = issue(uid, "alice", Role::Tech, None, &c).unwrap();
        let claims = verify(&token, &c).unwrap();
        assert_eq!(claims.sub, uid);
        assert_eq!(claims.username, "alice");
        assert_eq!(claims.role, Role::Tech);
        assert_eq!(claims.iss, c.jwt_issuer);
        assert_eq!(claims.aud, c.jwt_audience);
    }

    #[test]
    fn rejects_tampered_token() {
        let c = cfg();
        let token = issue(Uuid::new_v4(), "bob", Role::Admin, None, &c).unwrap();
        let tampered = format!("{}x", token);
        assert!(verify(&tampered, &c).is_err());
    }

    #[test]
    fn rejects_wrong_secret() {
        let mut c1 = cfg();
        let c2 = AuthConfig { jwt_secret: "different".into(), ..c1.clone() };
        c1.jwt_secret = "original".into();
        let token = issue(Uuid::new_v4(), "u", Role::Super, None, &c1).unwrap();
        assert!(verify(&token, &c2).is_err());
    }

    #[test]
    fn rejects_wrong_issuer() {
        let c = cfg();
        let token = issue(Uuid::new_v4(), "u", Role::Tech, None, &c).unwrap();
        let other = AuthConfig { jwt_issuer: "evil-service".into(), ..c.clone() };
        let err = verify(&token, &other).expect_err("issuer mismatch must be rejected");
        assert!(format!("{:?}", err).to_lowercase().contains("invalid"));
    }

    #[test]
    fn rejects_wrong_audience() {
        let c = cfg();
        let token = issue(Uuid::new_v4(), "u", Role::Tech, None, &c).unwrap();
        let other = AuthConfig { jwt_audience: "someone-elses-audience".into(), ..c.clone() };
        let err = verify(&token, &other).expect_err("audience mismatch must be rejected");
        assert!(format!("{:?}", err).to_lowercase().contains("invalid"));
    }
}
