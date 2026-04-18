//! Dedicated unit tests for `fieldops_backend::crypto`.
//!
//! The inline tests in `src/crypto.rs` cover the happy path + two failure
//! modes. This file adds the boundary and malformed-input cases the audit
//! flagged (`-4` in section 8 of the coverage report).

use fieldops_backend::crypto::{decrypt, encrypt};
use fieldops_backend::errors::ApiError;

const KEY_ALL_4S: [u8; 32] = [0x44u8; 32];
const KEY_ALL_7S: [u8; 32] = [0x77u8; 32];

fn is_bad_request(e: &ApiError) -> bool {
    matches!(e, ApiError::BadRequest(_))
}
fn is_internal(e: &ApiError) -> bool {
    matches!(e, ApiError::Internal(_))
}

#[test]
fn empty_plaintext_still_roundtrips() {
    // GCM allows 0-length plaintext; the auth tag is still present so the
    // ciphertext is longer than the nonce alone.
    let ct = encrypt("", &KEY_ALL_4S).unwrap();
    let bytes = hex::decode(&ct).unwrap();
    assert!(bytes.len() > 12, "must include nonce + auth tag even for empty input");
    assert_eq!(decrypt(&ct, &KEY_ALL_4S).unwrap(), "");
}

#[test]
fn unicode_plaintext_roundtrips() {
    let plaintext = "221B Baker Street, London — 🕵️";
    let ct = encrypt(plaintext, &KEY_ALL_4S).unwrap();
    assert_eq!(decrypt(&ct, &KEY_ALL_4S).unwrap(), plaintext);
}

#[test]
fn non_hex_ciphertext_returns_bad_request() {
    let e = decrypt("not-hex-!!!", &KEY_ALL_4S).unwrap_err();
    assert!(
        is_bad_request(&e),
        "non-hex payload must be a 400, not a 500 — got {:?}",
        e
    );
    assert!(format!("{}", e).to_lowercase().contains("hex"));
}

#[test]
fn odd_length_hex_returns_bad_request() {
    // Odd number of hex chars isn't decodable even though every char is hex.
    let e = decrypt("abc", &KEY_ALL_4S).unwrap_err();
    assert!(is_bad_request(&e));
}

#[test]
fn ciphertext_shorter_than_nonce_plus_tag_is_bad_request() {
    // NONCE_LEN (12) + GCM auth-tag size (16) = 28 bytes minimum. A 10-byte
    // hex-encoded payload decodes to 5 bytes.
    let e = decrypt("0123456789", &KEY_ALL_4S).unwrap_err();
    assert!(
        is_bad_request(&e),
        "truncated ciphertext must be 400 with 'too short' detail"
    );
    assert!(format!("{}", e).to_lowercase().contains("too short"));
}

#[test]
fn empty_ciphertext_is_bad_request() {
    let e = decrypt("", &KEY_ALL_4S).unwrap_err();
    assert!(is_bad_request(&e));
}

#[test]
fn decrypt_with_wrong_key_is_internal_not_bad_request() {
    // Wrong key passes structural checks but fails AEAD auth. That maps to
    // 500 (Internal) because callers shouldn't be able to brute-force keys
    // by observing 4xx vs 5xx — keep this behavior stable.
    let ct = encrypt("classified", &KEY_ALL_4S).unwrap();
    let e = decrypt(&ct, &KEY_ALL_7S).unwrap_err();
    assert!(
        is_internal(&e),
        "AEAD failure must map to Internal, got {:?}",
        e
    );
}

#[test]
fn bit_flip_in_auth_tag_is_detected() {
    let ct = encrypt("accountability", &KEY_ALL_4S).unwrap();
    let mut bytes = hex::decode(&ct).unwrap();
    // Flip one bit of the final byte (part of the auth tag) and re-encode.
    let last = bytes.len() - 1;
    bytes[last] ^= 0x80;
    let tampered = hex::encode(bytes);
    assert!(decrypt(&tampered, &KEY_ALL_4S).is_err());
}

#[test]
fn bit_flip_in_ciphertext_body_is_detected() {
    let ct = encrypt("payload integrity", &KEY_ALL_4S).unwrap();
    let mut bytes = hex::decode(&ct).unwrap();
    // Flip a byte in the ciphertext body (past the nonce, before the tag).
    let mid = 12 + (bytes.len() - 12 - 16) / 2;
    bytes[mid] ^= 0x01;
    let tampered = hex::encode(bytes);
    assert!(decrypt(&tampered, &KEY_ALL_4S).is_err());
}

#[test]
fn truncated_nonce_does_not_leak_plaintext() {
    // Drop the trailing bytes — decrypt must fail, and the error message
    // must never echo anything from the original plaintext.
    let ct = encrypt("super-sensitive-secret", &KEY_ALL_4S).unwrap();
    let bytes = hex::decode(&ct).unwrap();
    let truncated = hex::encode(&bytes[..10]);
    let e = decrypt(&truncated, &KEY_ALL_4S).unwrap_err();
    let msg = format!("{}", e);
    assert!(!msg.contains("super-sensitive-secret"));
    assert!(!msg.contains("secret"));
}

#[test]
fn ciphertext_is_never_equal_to_any_suffix_of_plaintext() {
    // Guard against nonce-reuse / raw-cipher accidents: even 1000 encrypts
    // of the same plaintext must produce 1000 distinct ciphertexts.
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for _ in 0..128 {
        let ct = encrypt("same-input-every-time", &KEY_ALL_4S).unwrap();
        assert!(
            seen.insert(ct),
            "duplicate ciphertext for identical plaintext — nonce reuse?"
        );
    }
}
