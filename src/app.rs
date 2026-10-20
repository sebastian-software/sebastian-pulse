use crate::{database, log, migrations::Migration};
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use serde_json::json;
use sqlx::PgPool;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use tokio::sync::{Mutex, OnceCell};

struct AppState {
    pool: PgPool,
    migrations: Vec<Migration>,
    release: String,
    draining: AtomicBool,
    // A probe's cell is shared by overlapping requests and removed after completion.
    pending_probe: Mutex<Option<Arc<OnceCell<bool>>>>,
    // 0 = not yet observed, 1 = unavailable, 2 = ready.
    previous_ready: AtomicU8,
}

#[derive(Clone)]
pub struct Service {
    state: Arc<AppState>,
}

impl Service {
    pub fn new(pool: PgPool, migrations: Vec<Migration>, release: String) -> Self {
        Self {
            state: Arc::new(AppState {
                pool,
                migrations,
                release,
                draining: AtomicBool::new(false),
                pending_probe: Mutex::new(None),
                previous_ready: AtomicU8::new(0),
            }),
        }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/health/live", axum::routing::get(live))
            .route("/health/ready", axum::routing::get(ready))
            .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({"error":"not_found"}))) })
            .layer(middleware::from_fn_with_state(self.state.clone(), guard))
            .with_state(self.state.clone())
    }

    pub fn drain(&self) {
        self.state.draining.store(true, Ordering::Release);
    }
}

async fn guard(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let mut response = if state.draining.load(Ordering::Acquire) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"draining", "release":state.release})),
        )
            .into_response()
    } else {
        match tokio::time::timeout(std::time::Duration::from_secs(10), next.run(request)).await {
            Ok(response) => response,
            Err(_) => (
                StatusCode::REQUEST_TIMEOUT,
                Json(json!({"error":"request_timeout"})),
            )
                .into_response(),
        }
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

async fn live(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({"status":"live", "release":state.release}))
}

async fn ready(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let probe = shared_probe(&state).await;
    let is_ready = *probe
        .get_or_init(|| async {
            match database::check_readiness(&state.pool, &state.migrations).await {
                Ok(ready) => ready,
                Err(error) => {
                    if state.previous_ready.load(Ordering::Acquire) != 1 {
                        log::event(
                            "database_probe_failed",
                            json!({"code": log::error_code(&error)}),
                        );
                    }
                    false
                }
            }
        })
        .await;
    {
        let mut pending = state.pending_probe.lock().await;
        if pending
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &probe))
        {
            *pending = None;
        }
    }
    let observed = if is_ready { 2 } else { 1 };
    if state.previous_ready.swap(observed, Ordering::AcqRel) != observed {
        log::event("readiness_changed", json!({"ready": u8::from(is_ready)}));
    }
    let available = is_ready && !state.draining.load(Ordering::Acquire);
    (
        if available {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(
            json!({"status": if available { "ready" } else { "unavailable" }, "release": state.release}),
        ),
    )
}

// A completed cell may remain if all request futures were canceled before cleanup.
// Only share an unfinished probe; the next request must never reuse that result.
async fn shared_probe(state: &AppState) -> Arc<OnceCell<bool>> {
    let mut pending = state.pending_probe.lock().await;
    if pending.as_ref().is_none_or(|probe| probe.initialized()) {
        *pending = Some(Arc::new(OnceCell::new()));
    }
    pending
        .as_ref()
        .expect("an unfinished probe was installed")
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgConnectOptions;

    #[tokio::test]
    async fn a_completed_probe_left_by_cancellation_is_not_reused() {
        let pool = database::create_pool(PgConnectOptions::new());
        let service = Service::new(pool.clone(), Vec::new(), "probe-test".into());
        let unfinished = shared_probe(&service.state).await;
        assert!(Arc::ptr_eq(
            &unfinished,
            &shared_probe(&service.state).await
        ));
        unfinished
            .set(true)
            .expect("the probe has not finished yet");
        // Model cancellation between probe completion and request cleanup.
        let next = shared_probe(&service.state).await;
        assert!(!Arc::ptr_eq(&unfinished, &next));
        assert!(!next.initialized());
        pool.close().await;
    }
}
