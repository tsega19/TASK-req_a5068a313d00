//! Permission-matrix spot checks — verifies every role hits each sensitive
//! route with the expected status code per PRD §9.
//!
//! Rule 3 of the test suite ("assert body content, not only status") is
//! satisfied in two layers: the route-specific test files assert on shape
//! and values; the focused tests below then cover body content for forbidden
//! and scoped RBAC paths that the table-driven matrix hits status-only.

use actix_web::test::TestRequest;

use super::common::{auth_header, json_of, make_service, raw_of, setup, status_of};

struct Case {
    method: &'static str,
    path: &'static str,
    token: &'static str,
    want: u16,
}

#[actix_web::test]
async fn rbac_matrix() {
    let ctx = setup().await;
    // token keys: "admin", "super", "tech_a", "tech_b"
    let cases = [
        // list work orders — everyone authed can hit; tech sees only own
        Case { method: "GET", path: "/api/work-orders", token: "tech_a", want: 200 },
        Case { method: "GET", path: "/api/work-orders", token: "super", want: 200 },
        Case { method: "GET", path: "/api/work-orders", token: "admin", want: 200 },

        // on-call queue — SUPER/ADMIN only
        Case { method: "GET", path: "/api/work-orders/on-call-queue", token: "tech_a", want: 403 },
        Case { method: "GET", path: "/api/work-orders/on-call-queue", token: "super", want: 200 },
        Case { method: "GET", path: "/api/work-orders/on-call-queue", token: "admin", want: 200 },

        // admin routes — ADMIN only
        Case { method: "GET", path: "/api/admin/users", token: "tech_a", want: 403 },
        Case { method: "GET", path: "/api/admin/users", token: "super", want: 403 },
        Case { method: "GET", path: "/api/admin/users", token: "admin", want: 200 },
        Case { method: "GET", path: "/api/admin/branches", token: "super", want: 403 },
        Case { method: "GET", path: "/api/admin/branches", token: "admin", want: 200 },

        // analytics — any authed role can hit (results are scoped)
        Case { method: "GET", path: "/api/analytics/learning", token: "tech_a", want: 200 },
        Case { method: "GET", path: "/api/analytics/learning", token: "super", want: 200 },
        Case { method: "GET", path: "/api/analytics/learning", token: "admin", want: 200 },
    ];

    for c in cases {
        let app = make_service(&ctx).await;
        let token = match c.token {
            "admin" => &ctx.admin_token,
            "super" => &ctx.super_token,
            "tech_a" => &ctx.tech_a_token,
            "tech_b" => &ctx.tech_b_token,
            _ => unreachable!(),
        };
        let req = match c.method {
            "GET" => TestRequest::get(),
            "POST" => TestRequest::post(),
            "PUT" => TestRequest::put(),
            "DELETE" => TestRequest::delete(),
            _ => unreachable!(),
        }
        .uri(c.path)
        .insert_header(auth_header(token))
        .to_request();
        let got = status_of(&app, req).await;
        assert_eq!(
            got, c.want,
            "{} {} as {} expected {} got {}",
            c.method, c.path, c.token, c.want, got
        );
    }
}

// -----------------------------------------------------------------------------
// Body-content assertions for the RBAC decision points the matrix covers
// status-only. Keeps rule 3 ("assert body, not just status") satisfied.
// -----------------------------------------------------------------------------

#[actix_web::test]
async fn forbidden_response_has_structured_body() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/admin/users")
        .insert_header(auth_header(&ctx.tech_a_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 403);
    // Every forbidden response surfaces the structured error envelope so
    // clients can branch on `code` rather than parsing the message.
    assert_eq!(body["code"], "forbidden");
    assert!(body["error"].as_str().is_some());
    assert!(!body["error"].as_str().unwrap().is_empty());
}

#[actix_web::test]
async fn unauthorized_response_has_structured_body() {
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get().uri("/api/work-orders").to_request();
    let (status, body) = raw_of(&app, req).await;
    assert_eq!(status, 401);
    assert_eq!(body["code"], "unauthorized");
    assert_eq!(body["error"], "missing bearer token");
}

#[actix_web::test]
async fn admin_users_list_body_contains_seeded_usernames() {
    // Strengthens the matrix's GET /api/admin/users "200 for admin" cell with
    // explicit body-content verification — the seeded admin/super/tech rows
    // must all appear in the paginated envelope.
    let ctx = setup().await;
    let app = make_service(&ctx).await;
    let req = TestRequest::get()
        .uri("/api/admin/users?per_page=50")
        .insert_header(auth_header(&ctx.admin_token))
        .to_request();
    let (status, body): (u16, serde_json::Value) = json_of(&app, req).await;
    assert_eq!(status, 200);
    // Envelope shape (pagination).
    assert!(body["page"].as_u64().is_some());
    assert!(body["per_page"].as_u64().is_some());
    assert!(body["total"].as_i64().is_some());
    let usernames: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["username"].as_str().unwrap())
        .collect();
    for expected in ["admin", "super_a", "tech_a", "tech_b"] {
        assert!(
            usernames.contains(&expected),
            "admin user list must contain '{}', got {:?}",
            expected,
            usernames
        );
    }
    // Secrets never leak: password_hash must be excluded from the JSON.
    for row in body["data"].as_array().unwrap() {
        assert!(
            row.get("password_hash").is_none(),
            "password_hash must never be serialized"
        );
    }
}
