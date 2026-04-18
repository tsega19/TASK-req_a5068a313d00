//! AES-256-GCM envelope encryption for sensitive fields at rest (PRD §10).
//!
//! The key comes from `AppConfig::encryption::aes_256_key` (32 bytes) — the
//! ONLY module that touches the raw key. Ciphertext layout:
//!
//! ```text
//! [12-byte nonce] ++ [GCM ciphertext including auth tag]
//! ```
//!
//! Stored in the database as a hex-encoded string so Postgres `TEXT` columns
//! remain portable. A fresh nonce is generated per encrypt call (OsRng).
//!
//! Used by the users path to store `home_address_enc` — plaintext never
//! touches the database, and log output never includes the plaintext.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::rngs::OsRng;
use rand::RngCore;

use crate::errors::ApiError;

const NONCE_LEN: usize = 12;

/// Encrypt `plaintext` under the configured AES-256 key. Returns a hex string
/// suitable for storage in a `TEXT` column.
pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> Result<String, ApiError> {
    let key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(key);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| ApiError::Internal(format!("encrypt failed: {}", e)))?;
    let mut combined = Vec::with_capacity(NONCE_LEN + ct.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ct);
    Ok(hex::encode(combined))
}

/// Decrypt a value previously produced by [`encrypt`]. Returns `ApiError` on
/// tampering (auth tag mismatch) or malformed input.
pub fn decrypt(ciphertext_hex: &str, key: &[u8; 32]) -> Result<String, ApiError> {
    let bytes = hex::decode(ciphertext_hex)
        .map_err(|e| ApiError::BadRequest(format!("bad ciphertext hex: {}", e)))?;
    if bytes.len() < NONCE_LEN + 16 {
        return Err(ApiError::BadRequest("ciphertext too short".into()));
    }
    let (nonce_bytes, ct) = bytes.split_at(NONCE_LEN);
    let key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);
    let pt = cipher
        .decrypt(nonce, ct)
        .map_err(|_| ApiError::Internal("decrypt failed (bad key or tampered ciphertext)".into()))?;
    String::from_utf8(pt).map_err(|e| ApiError::Internal(format!("utf8: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn roundtrip_preserves_plaintext() {
        let ct = encrypt("221B Baker Street", &key()).unwrap();
        let pt = decrypt(&ct, &key()).unwrap();
        assert_eq!(pt, "221B Baker Street");
    }

    #[test]
    fn distinct_nonce_per_encryption() {
        let a = encrypt("same", &key()).unwrap();
        let b = encrypt("same", &key()).unwrap();
        assert_ne!(a, b, "nonce reuse would produce identical ciphertext");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let ct = encrypt("secret", &key()).unwrap();
        let mut bytes = hex::decode(&ct).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let tampered = hex::encode(bytes);
        assert!(decrypt(&tampered, &key()).is_err());
    }

    #[test]
    fn wrong_key_is_rejected() {
        let ct = encrypt("secret", &key()).unwrap();
        let wrong_key = [0x33u8; 32];
        assert!(decrypt(&ct, &wrong_key).is_err());
    }

    #[test]
    fn ciphertext_is_not_plaintext() {
        let ct = encrypt("sensitive", &key()).unwrap();
        assert!(!ct.contains("sensitive"));
    }
}
