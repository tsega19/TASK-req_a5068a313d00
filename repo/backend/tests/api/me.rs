use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, json_of, make_service, raw_of, setup, status_of};

#[actix_web::test]
async fn get_me_returns_profile() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/me")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["username"], "tech_a");
    assert_eq!(body["role"], "TECH");
    assert_eq!(body["privacy_mode"], false);
}

#[actix_web::test]
async fn set_privacy_persists_value() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri("/api/me/privacy")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "privacy_mode": true }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["privacy_mode"], true);

    let actual: bool = sqlx::query_scalar("SELECT privacy_mode FROM users WHERE id = $1")
        .bind(ctx.tech_a_id)
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert!(actual);
}

#[actix_web::test]
async fn me_requires_auth() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get().uri("/api/me").to_request();
    assert_eq!(status_of(&app, req).await, 401);
}

#[actix_web::test]
async fn home_address_is_encrypted_at_rest() {
    let ctx = setup().await;

    // Write via the authenticated API.
    let app = make_service(&ctx).await;
    let plaintext = "221B Baker Street, London";
    let req = TestRequest::put()
        .uri("/api/me/home-address")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "home_address": plaintext }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["stored"], true);
    assert_eq!(body["home_address"], plaintext);

    // Read raw ciphertext straight from the database: the string stored
    // MUST NOT be the plaintext and MUST NOT contain any plaintext substring.
    let raw: String = sqlx::query_scalar("SELECT home_address_enc FROM users WHERE id = $1")
        .bind(ctx.tech_a_id)
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_ne!(raw, plaintext);
    assert!(!raw.contains("Baker"));
    assert!(!raw.contains("London"));
    // Hex-encoded output: all ASCII hex digits.
    assert!(raw.chars().all(|c| c.is_ascii_hexdigit()));

    // Read back via the authenticated API: plaintext round-trips.
    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::get()
        .uri("/api/me/home-address")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status2, body2) = raw_of(&app2, req2).await;
    assert_eq!(status2, 200);
    assert_eq!(body2["home_address"], plaintext);
}

#[actix_web::test]
async fn home_address_requires_auth() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri("/api/me/home-address")
        .set_json(json!({ "home_address": "anywhere" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 401);
}

#[actix_web::test]
async fn home_address_another_user_cannot_read() {
    let ctx = setup().await;

    // Tech A writes.
    let app = make_service(&ctx).await;
    let req = TestRequest::put()
        .uri("/api/me/home-address")
        .insert_header(auth_header(&ctx.tech_a_token))
        .set_json(json!({ "home_address": "A's address" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 200);

    // Tech B reads /api/me/home-address — they get THEIR row, which is empty,
    // not A's. Demonstrates per-user isolation of the decrypted plaintext.
    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::get()
        .uri("/api/me/home-address")
        .insert_header(auth_header(&ctx.tech_b_token))
        .to_request();
    let (status, body) = raw_of(&app2, req2).await;
    assert_eq!(status, 200);
    assert_eq!(body["stored"], false);
    assert!(body["home_address"].is_null());
}
