use actix_web::test::TestRequest;
use serde_json::json;

use super::common::{auth_header, json_of, make_service, raw_of, setup, status_of};

#[actix_web::test]
async fn health_is_public() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get().uri("/health").to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    // Rule 3: assert body shape, not only status.
    assert_eq!(body["status"], "ok");
}

#[actix_web::test]
async fn api_health_alternate_is_public_and_matches_shape() {
    // The audit flagged `/api/health` as untested despite being registered
    // separately from `/health`. Both must return the same envelope and must
    // bypass the JwtAuth middleware.
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get().uri("/api/health").to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
}

#[actix_web::test]
async fn login_success_returns_token_and_user() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": "admin", "password": "pw" }))
        .to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 200);
    assert!(body.get("token").and_then(|v| v.as_str()).is_some());
    assert_eq!(body["user"]["username"], "admin");
    assert_eq!(body["user"]["role"], "ADMIN");
}

#[actix_web::test]
async fn login_rejects_bad_password() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": "admin", "password": "wrong" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 401);
}

#[actix_web::test]
async fn login_rejects_unknown_user() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": "ghost", "password": "pw" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 401);
}

#[actix_web::test]
async fn login_rejects_empty_credentials() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": "", "password": "" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn logout_requires_bearer() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post().uri("/api/auth/logout").to_request();
    assert_eq!(status_of(&app, req).await, 401);
}

#[actix_web::test]
async fn logout_with_bearer_ok_and_advertises_stateless() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/auth/logout")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    assert_eq!(body["revoked"], false);
}

#[actix_web::test]
async fn protected_route_requires_bearer() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get().uri("/api/work-orders").to_request();
    assert_eq!(status_of(&app, req).await, 401);
}

#[actix_web::test]
async fn protected_route_rejects_invalid_token() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/work-orders")
        .insert_header(("Authorization", "Bearer not-a-real-token"))
        .to_request();
    assert_eq!(status_of(&app, req).await, 401);
}

#[actix_web::test]
async fn change_password_flips_reset_flag() {
    let ctx = setup().await;

    // Flag the admin row as needing a reset (simulates the seeded default
    // admin boot path).
    sqlx::query("UPDATE users SET password_reset_required = TRUE WHERE id = $1")
        .bind(ctx.admin_id)
        .execute(&ctx.pool)
        .await
        .unwrap();

    // Login should surface the flag.
    let app0 = make_service(&ctx).await;
    let login = TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": "admin", "password": "pw" }))
        .to_request();
    let (s0, body0) = raw_of(&app0, login).await;
    assert_eq!(s0, 200);
    assert_eq!(body0["password_reset_required"], true);

    // Rotate.
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/auth/change-password")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "current_password": "pw",
            "new_password": "a-brand-new-long-enough-password"
        }))
        .to_request();
    let (s, _) = raw_of(&app, req).await;
    assert_eq!(s, 200);

    // Re-login with the new password — flag should now be false.
    let app2 = make_service(&ctx).await;
    let req2 = TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({
            "username": "admin",
            "password": "a-brand-new-long-enough-password"
        }))
        .to_request();
    let (s2, body2) = raw_of(&app2, req2).await;
    assert_eq!(s2, 200);
    assert_eq!(body2["password_reset_required"], false);
}

#[actix_web::test]
async fn change_password_rejects_weak_password() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/auth/change-password")
        .insert_header(auth_header(&ctx.admin_token))
        .set_json(json!({
            "current_password": "pw",
            "new_password": "short"
        }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 400);
}

#[actix_web::test]
async fn change_password_requires_bearer() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::post()
        .uri("/api/auth/change-password")
        .set_json(json!({ "current_password": "x", "new_password": "yyyyyyyyyyyy" }))
        .to_request();
    assert_eq!(status_of(&app, req).await, 401);
}
