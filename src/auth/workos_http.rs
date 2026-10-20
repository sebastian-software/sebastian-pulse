use super::{
    AuthStore, Caller, http,
    workos::{BrowserIdentity, WorkosError, WorkosProvider},
};
use crate::workspaces::WorkspaceStore;
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub(super) struct WorkosState {
    provider: WorkosProvider,
    pub(super) store: AuthStore,
    workspaces: WorkspaceStore,
}
impl WorkosState {
    pub(super) fn new(pool: PgPool, provider: WorkosProvider) -> Self {
        Self {
            provider,
            store: AuthStore::new(pool.clone()),
            workspaces: WorkspaceStore::new(pool),
        }
    }
}

pub(super) fn router(state: Arc<WorkosState>) -> Router {
    let browser = http::configuration_router()
        .route_layer(middleware::from_fn_with_state(state.clone(), browser_auth))
        .with_state(state.workspaces.clone());
    let origin = state
        .provider
        .config()
        .public_url
        .origin()
        .ascii_serialization();
    let host = state.provider.config().public_url
        [url::Position::BeforeHost..url::Position::AfterPort]
        .to_owned();
    let mcp = crate::mcp::router(state.workspaces.clone(), host, origin)
        .route_layer(middleware::from_fn_with_state(state.clone(), bearer_auth));
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/session", get(session))
        .route("/auth/logout", post(logout))
        .route("/.well-known/oauth-protected-resource/mcp", get(metadata))
        .route("/.well-known/oauth-protected-resource", get(metadata))
        .with_state(state)
        .merge(browser)
        .merge(mcp)
        .layer(axum::extract::DefaultBodyLimit::max(65_536))
        .layer(middleware::from_fn(http::headers))
}
fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(json!({"error":code}))).into_response()
}
fn provider_error(err: WorkosError) -> Response {
    match err {
        WorkosError::Authentication => error(StatusCode::UNAUTHORIZED, "login_required"),
        WorkosError::Denied => error(StatusCode::FORBIDDEN, "access_denied"),
        _ => unavailable(),
    }
}
fn unavailable() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "authentication_unavailable",
    )
}
fn random() -> String {
    openidconnect::CsrfToken::new_random_len(32)
        .secret()
        .clone()
}
fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}
fn cookie_name(state: &WorkosState, kind: &str) -> String {
    format!(
        "{}pulse_{kind}",
        if state.provider.config().public_url.scheme() == "https" {
            "__Host-"
        } else {
            ""
        }
    )
}
fn cookie(
    state: &WorkosState,
    kind: &str,
    value: &str,
    max_age: u32,
) -> Result<HeaderValue, WorkosError> {
    if value.len() > 3800
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+' | b'/' | b'='))
    {
        return Err(WorkosError::Unavailable);
    }
    HeaderValue::from_str(&format!(
        "{}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        cookie_name(state, kind),
        if state.provider.config().public_url.scheme() == "https" {
            "; Secure"
        } else {
            ""
        }
    ))
    .map_err(|_| WorkosError::Unavailable)
}
fn read_cookie<'a>(state: &WorkosState, headers: &'a HeaderMap, kind: &str) -> Option<&'a str> {
    let name = cookie_name(state, kind);
    let mut found = None;
    for source in headers.get_all(header::COOKIE) {
        let source = source.to_str().ok()?;
        if source.len() > 16_384 {
            return None;
        }
        for part in source.split(';') {
            if let Some((key, value)) = part.trim().split_once('=')
                && key == name
            {
                if found.is_some()
                    || !(32..=3800).contains(&value.len())
                    || !value.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+' | b'/' | b'=')
                    })
                {
                    return None;
                }
                found = Some(value);
            }
        }
    }
    found
}
fn csrf_valid(state: &WorkosState, headers: &HeaderMap) -> bool {
    let origin = state
        .provider
        .config()
        .public_url
        .origin()
        .ascii_serialization();
    headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(origin.as_str())
        && read_cookie(state, headers, "csrf").is_some_and(|expected| {
            headers
                .get("x-csrf-token")
                .is_some_and(|v| bool::from(v.as_bytes().ct_eq(expected.as_bytes())))
        })
}
fn renew(state: &WorkosState, identity: &BrowserIdentity, mut response: Response) -> Response {
    if let Some(sealed) = &identity.renewed_session {
        match cookie(state, "session", sealed.expose(), 604_800) {
            Ok(cookie) => {
                response.headers_mut().append(header::SET_COOKIE, cookie);
            }
            Err(error) => return provider_error(error),
        }
    }
    response
}
async fn login(State(state): State<Arc<WorkosState>>) -> Response {
    let request = match state.provider.begin_login() {
        Ok(v) => v,
        Err(e) => return provider_error(e),
    };
    let binding = random();
    if state
        .store
        .begin_login(
            &hash(&request.state),
            &hash(&binding),
            request.verifier.expose(),
            &request.state,
        )
        .await
        .is_err()
    {
        return unavailable();
    }
    let mut response = Redirect::to(&request.url).into_response();
    match cookie(&state, "login", &binding, 600) {
        Ok(v) => {
            response.headers_mut().append(header::SET_COOKIE, v);
        }
        Err(e) => return provider_error(e),
    };
    response
}
#[derive(Deserialize)]
struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}
async fn callback(
    State(state): State<Arc<WorkosState>>,
    headers: HeaderMap,
    Query(input): Query<Callback>,
) -> Response {
    let Some(binding) = read_cookie(&state, &headers, "login") else {
        return error(StatusCode::BAD_REQUEST, "invalid_login");
    };
    let Some(csrf) = input.state.filter(|v| !v.is_empty() && v.len() <= 128) else {
        return error(StatusCode::BAD_REQUEST, "invalid_login");
    };
    let attempt = match state
        .store
        .consume_login(&hash(&csrf), &hash(binding))
        .await
    {
        Ok(Some(v)) => v,
        Ok(None) => return error(StatusCode::BAD_REQUEST, "invalid_login"),
        Err(_) => return unavailable(),
    };
    let mut response = if input.error.is_some() {
        error(StatusCode::BAD_REQUEST, "login_rejected")
    } else if let Some(code) = input.code.filter(|v| !v.is_empty() && v.len() <= 4096) {
        match state
            .provider
            .finish_login(code, attempt.pkce_verifier.into())
            .await
        {
            Ok(logged_in) => {
                let mut response = Redirect::to("/auth/session").into_response();
                let session = match cookie(
                    &state,
                    "session",
                    logged_in.sealed_session.expose(),
                    604_800,
                ) {
                    Ok(v) => v,
                    Err(e) => return provider_error(e),
                };
                let csrf = match cookie(&state, "csrf", &random(), 604_800) {
                    Ok(v) => v,
                    Err(e) => return provider_error(e),
                };
                response.headers_mut().append(header::SET_COOKIE, session);
                response.headers_mut().append(header::SET_COOKIE, csrf);
                response
            }
            Err(e) => provider_error(e),
        }
    } else {
        error(StatusCode::BAD_REQUEST, "invalid_login")
    };
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie(&state, "login", "", 0).expect("empty cookie"),
    );
    response
}
async fn identity(state: &WorkosState, headers: &HeaderMap) -> Result<BrowserIdentity, Response> {
    let sealed = read_cookie(state, headers, "session")
        .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "login_required"))?;
    match state.provider.browser_identity(sealed).await {
        Ok(identity) => Ok(identity),
        Err(WorkosError::Authentication) => {
            let mut response = provider_error(WorkosError::Authentication);
            for kind in ["session", "csrf"] {
                response.headers_mut().append(
                    header::SET_COOKIE,
                    cookie(state, kind, "", 0).expect("empty cookie"),
                );
            }
            Err(response)
        }
        Err(error) => Err(provider_error(error)),
    }
}
async fn caller(state: &WorkosState, user: &str, organization: &str) -> Result<Caller, Response> {
    let access = state
        .provider
        .permissions(user, organization)
        .await
        .map_err(provider_error)?;
    state
        .store
        .workos_caller(&state.provider.config().client_id, access)
        .await
        .map_err(|_| unavailable())?
        .ok_or_else(|| error(StatusCode::FORBIDDEN, "workspace_not_bound"))
}
async fn session(State(state): State<Arc<WorkosState>>, headers: HeaderMap) -> Response {
    let identity = match identity(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(csrf) = read_cookie(&state, &headers, "csrf") else {
        return error(StatusCode::UNAUTHORIZED, "login_required");
    };
    let caller = match caller(&state, &identity.user_id, &identity.organization_id).await {
        Ok(v) => Some(v),
        Err(e) if e.status() == StatusCode::FORBIDDEN => None,
        Err(e) => return renew(&state, &identity, e),
    };
    let response = Json(json!({
        "authenticated": true,
        "user_id": identity.user_id,
        "organization_id": identity.organization_id,
        "principal_id": caller.map(|v| v.principal_id),
        "workspace_id": caller.and_then(|v| v.workspace_id),
        "can_write": caller.is_some_and(|v| v.can_write),
        "csrf_token": csrf,
    }))
    .into_response();
    renew(&state, &identity, response)
}
async fn logout(State(state): State<Arc<WorkosState>>, headers: HeaderMap) -> Response {
    if !csrf_valid(&state, &headers) {
        return error(StatusCode::FORBIDDEN, "invalid_csrf");
    }
    let identity = match identity(&state, &headers).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut response = match state.provider.revoke_session(&identity.session_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => provider_error(e),
    };
    for kind in ["session", "csrf", "login"] {
        response.headers_mut().append(
            header::SET_COOKIE,
            cookie(&state, kind, "", 0).expect("empty cookie"),
        );
    }
    response
}
async fn browser_auth(
    State(state): State<Arc<WorkosState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity = match identity(&state, request.headers()).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !matches!(*request.method(), Method::GET | Method::HEAD)
        && !csrf_valid(&state, request.headers())
    {
        return renew(
            &state,
            &identity,
            error(StatusCode::FORBIDDEN, "invalid_csrf"),
        );
    }
    let caller = match caller(&state, &identity.user_id, &identity.organization_id).await {
        Ok(v) => v,
        Err(e) => return renew(&state, &identity, e),
    };
    if !matches!(*request.method(), Method::GET | Method::HEAD) && !caller.can_write {
        return renew(
            &state,
            &identity,
            error(StatusCode::FORBIDDEN, "access_denied"),
        );
    }
    request.extensions_mut().insert(caller);
    renew(&state, &identity, next.run(request).await)
}
async fn metadata(State(state): State<Arc<WorkosState>>) -> Json<serde_json::Value> {
    Json(
        json!({"resource":state.provider.config().resource(),"authorization_servers":[state.provider.config().issuer()],"bearer_methods_supported":["header"]}),
    )
}
async fn bearer_auth(
    State(state): State<Arc<WorkosState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let config = state.provider.config();
    let host = &config.public_url[url::Position::BeforeHost..url::Position::AfterPort];
    let origin = config.public_url.origin().ascii_serialization();
    if request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        != Some(host)
        || request
            .headers()
            .get(header::ORIGIN)
            .is_some_and(|v| v.to_str().ok() != Some(origin.as_str()))
    {
        return error(StatusCode::FORBIDDEN, "invalid_origin");
    }
    let challenge = || {
        let mut response = error(StatusCode::UNAUTHORIZED, "invalid_bearer_token");
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_str(&format!(
                "Bearer resource_metadata=\"{}\"",
                config
                    .public_url
                    .join(".well-known/oauth-protected-resource/mcp")
                    .expect("validated origin")
            ))
            .expect("validated origin"),
        );
        response
    };
    if request
        .headers()
        .get_all(header::AUTHORIZATION)
        .iter()
        .count()
        != 1
    {
        return challenge();
    }
    let Some(token) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return challenge();
    };
    let access = match state.provider.verify_bearer(token).await {
        Ok(v) => v,
        Err(WorkosError::Authentication) => return challenge(),
        Err(e) => return provider_error(e),
    };
    let caller = match state.store.workos_caller(&config.client_id, access).await {
        Ok(Some(v)) => v,
        Ok(None) => return error(StatusCode::FORBIDDEN, "workspace_not_bound"),
        Err(_) => return unavailable(),
    };
    request.extensions_mut().insert(caller);
    next.run(request).await
}
