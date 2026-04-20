//! Argon2id password hashing. Parameters come from `AppConfig::auth` only.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::rngs::OsRng;

use crate::config::AuthConfig;
use crate::errors::ApiError;

fn build_argon2(cfg: &AuthConfig) -> Argon2<'static> {
    let params = Params::new(
        cfg.argon2_memory_kib,
        cfg.argon2_iterations,
        cfg.argon2_parallelism,
        None,
    )
    .expect("valid argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(pw: &str, cfg: &AuthConfig) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon = build_argon2(cfg);
    argon
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::Internal(format!("hash error: {}", e)))
}

pub fn verify_password(pw: &str, stored_hash: &str) -> Result<bool, ApiError> {
    let parsed = PasswordHash::new(stored_hash)
        .map_err(|e| ApiError::Internal(format!("hash parse: {}", e)))?;
    Ok(Argon2::default()
        .verify_password(pw.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AuthConfig {
        AuthConfig {
            jwt_secret: "x".into(),
            jwt_expiry_hours: 24,
            jwt_issuer: "test".into(),
            jwt_audience: "test".into(),
            argon2_memory_kib: 19456,
            argon2_iterations: 2,
            argon2_parallelism: 1,
        }
    }

    #[test]
    fn roundtrip_verifies() {
        let h = hash_password("hunter2", &cfg()).unwrap();
        assert!(verify_password("hunter2", &h).unwrap());
        assert!(!verify_password("wrong", &h).unwrap());
    }

    #[test]
    fn hash_is_argon2id() {
        let h = hash_password("pw", &cfg()).unwrap();
        assert!(h.starts_with("$argon2id$"));
    }
}
