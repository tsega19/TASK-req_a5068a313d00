//! Offline-first client (PRD §8).
//!
//! Provides three primitives the rest of the UI relies on to keep working in
//! the field when the network drops:
//!
//!   1. A write-through **cache** in localStorage keyed by the request path —
//!      successful GETs are mirrored so subsequent reads can be served from
//!      disk while offline.
//!   2. A persistent **mutation queue** — PUT/POST/DELETE bodies that couldn't
//!      reach the server are serialized to localStorage and replayed in FIFO
//!      order when connectivity returns.
//!   3. A **sync loop** that pulls `/api/sync/changes` on a timer, keeps a
//!      cursor in localStorage, and drains the queue against
//!      `/api/sync/step-progress` (and other push endpoints).
//!
//! Conflict handling is delegated to the backend's deterministic merge
//! (`sync::merge`). Outcomes come back on the mutation responses; the queue
//! drops rows the server has accepted and keeps the rest for retry.

use gloo_net::http::Method;
use gloo_storage::{LocalStorage, Storage};
use gloo_timers::callback::Interval;
use serde::{Deserialize, Serialize};
use web_sys::window;

use crate::api::{self, ApiError};
use crate::auth::AuthState;

const KEY_QUEUE: &str = "fieldops_offline_queue";
const KEY_CACHE_PREFIX: &str = "fieldops_cache::";
const KEY_SYNC_CURSOR: &str = "fieldops_sync_cursor";
const KEY_NEXT_ID: &str = "fieldops_offline_next_id";

/// How often the background sync loop fires (ms). Short enough that a
/// reconnected tablet converges quickly, long enough that an idle tab doesn't
/// hammer the API.
pub const SYNC_INTERVAL_MS: u32 = 15_000;

/// Maximum retry attempts per queued mutation before it is moved to the
/// dead-letter slot (still visible to the user, just no longer auto-retried).
pub const MAX_QUEUE_ATTEMPTS: u32 = 12;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct QueuedMutation {
    pub id: u64,
    pub method: String,
    pub path: String,
    pub body: serde_json::Value,
    /// Epoch millis when the mutation was enqueued (client clock).
    pub queued_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// Snapshot of the offline-client state — shown in the UI so the user knows
/// whether they are offline and how many mutations are pending.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OfflineStatus {
    pub online: bool,
    pub queue_depth: usize,
    pub dead_letter_depth: usize,
}

// ---------------------------------------------------------------------------
// Online detection
// ---------------------------------------------------------------------------

/// Reads `navigator.onLine`. Defaults to `true` in environments where the
/// navigator isn't available (SSR, tests) — "assume reachable" keeps the
/// happy path smooth while the queue still buffers any failure.
pub fn is_online() -> bool {
    window()
        .map(|w| w.navigator().on_line())
        .unwrap_or(true)
}

// ---------------------------------------------------------------------------
// Cache (GET results)
// ---------------------------------------------------------------------------

fn cache_key(path: &str) -> String {
    format!("{}{}", KEY_CACHE_PREFIX, path)
}

pub fn cache_put<T: Serialize>(path: &str, value: &T) {
    let _ = LocalStorage::set(&cache_key(path), value);
}

pub fn cache_get<T: serde::de::DeserializeOwned>(path: &str) -> Option<T> {
    LocalStorage::get(&cache_key(path)).ok()
}

pub fn cache_drop(path: &str) {
    LocalStorage::delete(&cache_key(path));
}

// ---------------------------------------------------------------------------
// Mutation queue
// ---------------------------------------------------------------------------

fn next_id() -> u64 {
    let n: u64 = LocalStorage::get(KEY_NEXT_ID).unwrap_or(1);
    let _ = LocalStorage::set(KEY_NEXT_ID, n.wrapping_add(1));
    n
}

pub fn queue() -> Vec<QueuedMutation> {
    LocalStorage::get(KEY_QUEUE).unwrap_or_default()
}

pub fn queue_depth() -> usize {
    queue().len()
}

pub fn status() -> OfflineStatus {
    let q = queue();
    let dead = q.iter().filter(|m| m.attempts >= MAX_QUEUE_ATTEMPTS).count();
    OfflineStatus {
        online: is_online(),
        queue_depth: q.len(),
        dead_letter_depth: dead,
    }
}

fn write_queue(q: &[QueuedMutation]) {
    let _ = LocalStorage::set(KEY_QUEUE, q);
}

pub fn enqueue(method: Method, path: &str, body: serde_json::Value) -> QueuedMutation {
    let m = QueuedMutation {
        id: next_id(),
        method: method_label(method).to_string(),
        path: path.to_string(),
        body,
        queued_at: now_millis(),
        attempts: 0,
        last_error: None,
    };
    let mut q = queue();
    q.push(m.clone());
    write_queue(&q);
    m
}

fn method_label(m: Method) -> &'static str {
    match m {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        _ => "POST",
    }
}

fn method_from_label(s: &str) -> Method {
    match s {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        _ => Method::POST,
    }
}

/// Attempt to push every queued mutation to the server in FIFO order. Stops
/// on the first network error (so a dropped connection doesn't silently mark
/// subsequent writes as failing too). Returns the number of rows drained.
pub async fn flush_queue(auth: &AuthState) -> usize {
    if !is_online() {
        return 0;
    }
    let mut q = queue();
    let mut drained = 0usize;
    let mut i = 0usize;
    while i < q.len() {
        if q[i].attempts >= MAX_QUEUE_ATTEMPTS {
            // Dead-letter row — skip but keep for user-visible surfacing.
            i += 1;
            continue;
        }
        let m = q[i].clone();
        let method = method_from_label(&m.method);
        let res: Result<serde_json::Value, ApiError> = if method == Method::DELETE {
            api::delete_(&m.path, auth)
                .await
                .map(|_| serde_json::Value::Null)
        } else {
            api::send_json::<serde_json::Value, serde_json::Value>(
                method, &m.path, &m.body, auth,
            )
            .await
        };
        match res {
            Ok(_) => {
                q.remove(i);
                drained += 1;
                write_queue(&q);
            }
            Err(e) => {
                q[i].attempts += 1;
                q[i].last_error = Some(e.message.clone());
                write_queue(&q);
                // Network errors (status==0) mean we are offline again — stop.
                if e.status == 0 {
                    break;
                }
                // Non-retryable 4xx (except 408/429) — drop so we don't spin.
                if (400..500).contains(&e.status) && e.status != 408 && e.status != 429 {
                    q.remove(i);
                    write_queue(&q);
                    continue;
                }
                i += 1;
            }
        }
    }
    drained
}

// ---------------------------------------------------------------------------
// Sync cursor + pull
// ---------------------------------------------------------------------------

pub fn get_sync_cursor() -> Option<String> {
    LocalStorage::get(KEY_SYNC_CURSOR).ok()
}

pub fn set_sync_cursor(s: &str) {
    let _ = LocalStorage::set(KEY_SYNC_CURSOR, s);
}

pub fn clear_sync_cursor() {
    LocalStorage::delete(KEY_SYNC_CURSOR);
}

/// Pull change tombstones since the last known cursor and advance the cursor.
/// Cache entries referenced by a DELETE/UPDATE are invalidated so subsequent
/// reads refresh from the server.
pub async fn pull_changes(auth: &AuthState) -> Result<usize, ApiError> {
    if !is_online() {
        return Ok(0);
    }
    let path = match get_sync_cursor() {
        Some(s) => format!("/api/sync/changes?since={}", encode_uri(&s)),
        None => "/api/sync/changes".to_string(),
    };
    let resp: serde_json::Value = api::get(&path, auth).await?;
    let count = resp
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    if let Some(c) = resp.get("next_cursor").and_then(|v| v.as_str()) {
        set_sync_cursor(c);
    }
    if let Some(arr) = resp.get("data").and_then(|v| v.as_array()) {
        for row in arr {
            let table = row.get("entity_table").and_then(|v| v.as_str()).unwrap_or("");
            let id = row.get("entity_id").and_then(|v| v.as_str()).unwrap_or("");
            // Invalidate every cached path that mentions this entity. Simple
            // substring match is good enough for the handful of paths we cache.
            invalidate_by_entity(table, id);
        }
    }
    Ok(count)
}

fn invalidate_by_entity(table: &str, id: &str) {
    let prefix = format!("/api/{}", table.trim_end_matches('s')); // singular-ish
    let also = format!("/api/{}", table);
    let touched: Vec<String> = all_cache_keys()
        .into_iter()
        .filter(|k| k.contains(id) || k.starts_with(&cache_key(&prefix)) || k.starts_with(&cache_key(&also)))
        .collect();
    for k in touched {
        LocalStorage::delete(&k);
    }
}

fn all_cache_keys() -> Vec<String> {
    let Some(w) = window() else { return vec![]; };
    let Ok(storage_opt) = w.local_storage() else { return vec![]; };
    let Some(storage) = storage_opt else { return vec![]; };
    let mut out = Vec::new();
    let len = storage.length().unwrap_or(0);
    for i in 0..len {
        if let Ok(Some(k)) = storage.key(i) {
            if k.starts_with(KEY_CACHE_PREFIX) {
                out.push(k);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Sync loop
// ---------------------------------------------------------------------------

/// Start a background sync loop that repeatedly flushes the queue and pulls
/// change tombstones. The returned `Interval` must be kept alive by the
/// caller (e.g. stored in an Rc inside the root component) — dropping it
/// cancels the loop.
pub fn start_sync_loop(auth: AuthState) -> Interval {
    // Kick once immediately so a freshly-loaded tab converges without waiting
    // an entire interval.
    let kick_auth = auth.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = pull_changes(&kick_auth).await;
        let _ = flush_queue(&kick_auth).await;
    });
    Interval::new(SYNC_INTERVAL_MS, move || {
        let auth = auth.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if !is_online() {
                return;
            }
            let _ = flush_queue(&auth).await;
            let _ = pull_changes(&auth).await;
        });
    })
}

// ---------------------------------------------------------------------------
// Offline-aware GET / mutation wrappers
// ---------------------------------------------------------------------------

/// Offline-aware GET: caches successful responses and falls back to the cache
/// when the network errors out. Returns `ApiError` only when both the network
/// AND the cache miss — giving the UI one consistent failure path to render.
pub async fn get_cached<T>(path: &str, auth: &AuthState) -> Result<T, ApiError>
where
    T: serde::de::DeserializeOwned + Serialize,
{
    if !is_online() {
        if let Some(v) = cache_get::<T>(path) {
            return Ok(v);
        }
    }
    match api::get::<serde_json::Value>(path, auth).await {
        Ok(v) => {
            cache_put(path, &v);
            serde_json::from_value::<T>(v).map_err(|e| ApiError {
                status: 0,
                message: format!("decode: {}", e),
                code: "parse".into(),
            })
        }
        Err(e) if e.status == 0 => {
            if let Some(v) = cache_get::<T>(path) {
                Ok(v)
            } else {
                Err(e)
            }
        }
        Err(e) => Err(e),
    }
}

/// Offline-aware mutation: goes straight to the network when online; when
/// offline (or the request fails with a network error) the body is enqueued
/// for replay and the caller receives `Ok(None)` so the UI can render an
/// optimistic success state.
pub async fn mutate_with_queue(
    method: Method,
    path: &str,
    body: &serde_json::Value,
    auth: &AuthState,
) -> Result<Option<serde_json::Value>, ApiError> {
    if !is_online() {
        enqueue(method, path, body.clone());
        return Ok(None);
    }
    match api::send_json::<_, serde_json::Value>(method.clone(), path, body, auth).await {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.status == 0 => {
            enqueue(method, path, body.clone());
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------

fn now_millis() -> i64 {
    js_sys::Date::now() as i64
}

fn encode_uri(s: &str) -> String {
    // Minimal encoder for the characters that appear in an RFC3339 cursor and
    // require escaping in a URL query string. Sidesteps the wasm-bindgen /
    // js-sys API surface so this module compiles cleanly against any version.
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            _ => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // `wasm_bindgen_test_configure!(run_in_browser)` lives in types.rs — it's
    // a crate-wide signal and declaring it here would duplicate the no_mangle
    // symbol wasm-bindgen-test emits.

    #[wasm_bindgen_test]
    fn method_label_roundtrip() {
        for m in [Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            let s = method_label(m.clone());
            assert_eq!(method_label(method_from_label(s)), s);
        }
    }

    #[wasm_bindgen_test]
    fn queued_mutation_serializes() {
        let m = QueuedMutation {
            id: 1,
            method: "PUT".into(),
            path: "/api/foo".into(),
            body: serde_json::json!({"x": 1}),
            queued_at: 0,
            attempts: 0,
            last_error: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: QueuedMutation = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[wasm_bindgen_test]
    fn encode_uri_escapes_non_url_safe_chars() {
        // RFC3339 cursors contain `:` and `+` which MUST be percent-encoded
        // before they go in a query string. Regression guard for the offline
        // sync-cursor round-trip.
        assert_eq!(encode_uri("a-b_c.~0"), "a-b_c.~0");
        assert_eq!(encode_uri("2026-04-20T12:30:00Z"), "2026-04-20T12%3A30%3A00Z");
    }

    #[wasm_bindgen_test]
    fn enqueue_then_queue_returns_row() {
        // Drain pre-existing state so the test is deterministic across runs
        // in the same browser context.
        gloo_storage::LocalStorage::delete(KEY_QUEUE);
        let m = enqueue(Method::PUT, "/api/unit-test", serde_json::json!({"x": 1}));
        let q = queue();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].id, m.id);
        assert_eq!(q[0].method, "PUT");
        assert_eq!(q[0].path, "/api/unit-test");
        gloo_storage::LocalStorage::delete(KEY_QUEUE);
    }

    #[wasm_bindgen_test]
    fn status_flags_dead_letter_rows() {
        gloo_storage::LocalStorage::delete(KEY_QUEUE);
        // Seed a row whose attempts exceed the retry cap.
        let bad = QueuedMutation {
            id: 999,
            method: "POST".into(),
            path: "/api/will-never-succeed".into(),
            body: serde_json::json!({}),
            queued_at: 0,
            attempts: MAX_QUEUE_ATTEMPTS + 1,
            last_error: Some("boom".into()),
        };
        write_queue(&[bad]);
        let s = status();
        assert_eq!(s.queue_depth, 1);
        assert_eq!(s.dead_letter_depth, 1);
        gloo_storage::LocalStorage::delete(KEY_QUEUE);
    }
}
