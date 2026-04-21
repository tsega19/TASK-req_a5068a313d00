//! Fetch wrapper. Attaches bearer token, parses `{error, code}` bodies into
//! human-readable messages, returns typed responses via serde.

use gloo_net::http::{Method, Request, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::auth::AuthState;

#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    pub code: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (HTTP {})", self.message, self.status)
    }
}

fn base() -> String {
    // Relative URL — nginx proxies /api/* to the backend service.
    String::new()
}

fn with_auth(mut b: RequestBuilder, auth: &AuthState) -> RequestBuilder {
    if let Some(token) = &auth.token {
        b = b.header("Authorization", &format!("Bearer {}", token));
    }
    b
}

async fn parse<T: DeserializeOwned>(resp: gloo_net::http::Response) -> Result<T, ApiError> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if (200..300).contains(&status) {
        if text.is_empty() {
            serde_json::from_str("null")
                .map_err(|e| ApiError { status, message: e.to_string(), code: "parse".into() })
        } else {
            serde_json::from_str::<T>(&text).map_err(|e| ApiError {
                status,
                message: format!("decode: {}", e),
                code: "parse".into(),
            })
        }
    } else {
        let (msg, code) = match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => (
                v.get("error").and_then(|x| x.as_str()).unwrap_or(&text).to_string(),
                v.get("code").and_then(|x| x.as_str()).unwrap_or("error").to_string(),
            ),
            Err(_) => (text, "error".into()),
        };
        Err(ApiError { status, message: msg, code })
    }
}

pub async fn get<T: DeserializeOwned>(path: &str, auth: &AuthState) -> Result<T, ApiError> {
    let url = format!("{}{}", base(), path);
    let req = with_auth(Request::get(&url), auth)
        .build()
        .map_err(|e| ApiError { status: 0, message: e.to_string(), code: "build".into() })?;
    let resp = req.send().await.map_err(|e| ApiError {
        status: 0,
        message: e.to_string(),
        code: "network".into(),
    })?;
    parse(resp).await
}

pub async fn send_json<B: Serialize, T: DeserializeOwned>(
    method: Method,
    path: &str,
    body: &B,
    auth: &AuthState,
) -> Result<T, ApiError> {
    send_json_with_if_match(method, path, body, auth, None).await
}

/// Like `send_json`, but attaches an `If-Match` header when one is supplied.
/// Mutating work-order endpoints require this for PRD §8 optimistic
/// concurrency (audit-2 High #3).
pub async fn send_json_with_if_match<B: Serialize, T: DeserializeOwned>(
    method: Method,
    path: &str,
    body: &B,
    auth: &AuthState,
    if_match: Option<&str>,
) -> Result<T, ApiError> {
    let url = format!("{}{}", base(), path);
    let builder = match method {
        Method::POST => Request::post(&url),
        Method::PUT => Request::put(&url),
        Method::DELETE => Request::delete(&url),
        Method::PATCH => Request::patch(&url),
        _ => Request::get(&url),
    };
    let mut builder = with_auth(builder, auth).header("Content-Type", "application/json");
    if let Some(etag) = if_match {
        builder = builder.header("If-Match", etag);
    }
    let req = builder
        .json(body)
        .map_err(|e| ApiError { status: 0, message: e.to_string(), code: "serialize".into() })?;
    let resp = req.send().await.map_err(|e| ApiError {
        status: 0,
        message: e.to_string(),
        code: "network".into(),
    })?;
    parse(resp).await
}

pub async fn post<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
    auth: &AuthState,
) -> Result<T, ApiError> {
    send_json(Method::POST, path, body, auth).await
}

pub async fn put<B: Serialize, T: DeserializeOwned>(
    path: &str,
    body: &B,
    auth: &AuthState,
) -> Result<T, ApiError> {
    send_json(Method::PUT, path, body, auth).await
}

pub async fn delete_(path: &str, auth: &AuthState) -> Result<(), ApiError> {
    let url = format!("{}{}", base(), path);
    let req = with_auth(Request::delete(&url), auth)
        .build()
        .map_err(|e| ApiError { status: 0, message: e.to_string(), code: "build".into() })?;
    let resp = req.send().await.map_err(|e| ApiError {
        status: 0,
        message: e.to_string(),
        code: "network".into(),
    })?;
    let status = resp.status();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        let text = resp.text().await.unwrap_or_default();
        Err(ApiError {
            status,
            message: text,
            code: "error".into(),
        })
    }
}

/// Fetch a URL and return the raw body as bytes — used by CSV export.
pub async fn get_bytes(path: &str, auth: &AuthState) -> Result<Vec<u8>, ApiError> {
    let url = format!("{}{}", base(), path);
    let req = with_auth(Request::get(&url), auth)
        .build()
        .map_err(|e| ApiError { status: 0, message: e.to_string(), code: "build".into() })?;
    let resp = req.send().await.map_err(|e| ApiError {
        status: 0,
        message: e.to_string(),
        code: "network".into(),
    })?;
    let status = resp.status();
    if !(200..300).contains(&status) {
        let text = resp.text().await.unwrap_or_default();
        return Err(ApiError { status, message: text, code: "error".into() });
    }
    resp.binary().await.map_err(|e| ApiError {
        status,
        message: e.to_string(),
        code: "decode".into(),
    })
}
