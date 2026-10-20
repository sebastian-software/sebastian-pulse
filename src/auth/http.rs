use super::{AuthState, Caller};
use crate::workspaces::{NewGoal, NewProject, NewRecipient, WorkspaceError};
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post, put},
};
use openidconnect::CsrfToken;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub(super) fn router(state: Arc<AuthState>) -> Router {
    let browser = configuration_router()
        .route_layer(middleware::from_fn_with_state(state.clone(), browser_auth))
        .with_state(state.workspaces.clone());
    let mcp = crate::mcp::router(
        state.workspaces.clone(),
        state.provider.config().public_url[url::Position::BeforeHost..url::Position::AfterPort]
            .to_owned(),
        state
            .provider
            .config()
            .public_url
            .origin()
            .ascii_serialization(),
    )
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
        .layer(middleware::from_fn(headers))
}

pub(super) fn configuration_router() -> Router<crate::workspaces::WorkspaceStore> {
    Router::new()
        .route("/v1/workspaces/{workspace}", get(workspace))
        .route(
            "/v1/workspaces/{workspace}/projects",
            get(projects).post(create_project),
        )
        .route(
            "/v1/workspaces/{workspace}/projects/{id}",
            put(update_project).delete(delete_project),
        )
        .route(
            "/v1/workspaces/{workspace}/goals",
            get(goals).post(create_goal),
        )
        .route(
            "/v1/workspaces/{workspace}/goals/{id}",
            put(update_goal).delete(delete_goal),
        )
        .route(
            "/v1/workspaces/{workspace}/recipients",
            get(recipients).post(create_recipient),
        )
        .route(
            "/v1/workspaces/{workspace}/recipients/{id}",
            put(update_recipient).delete(delete_recipient),
        )
}

pub(super) async fn headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(json!({"error":code}))).into_response()
}
fn unavailable() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "authentication_unavailable",
    )
}
fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}
fn random() -> String {
    CsrfToken::new_random_len(32).secret().clone()
}
fn cookie_name(state: &AuthState, login: bool) -> &'static str {
    match (state.provider.config().secure_cookies(), login) {
        (true, true) => "__Host-analytics_login",
        (true, false) => "__Host-analytics_session",
        (false, true) => "analytics_login",
        (false, false) => "analytics_session",
    }
}
fn cookie(state: &AuthState, login: bool, value: &str, max_age: u16) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        cookie_name(state, login),
        if state.provider.config().secure_cookies() {
            "; Secure"
        } else {
            ""
        }
    ))
    .expect("only generated URL-safe tokens are used in cookies")
}
fn read_cookie<'a>(state: &AuthState, headers: &'a HeaderMap, login: bool) -> Option<&'a str> {
    let name = cookie_name(state, login);
    let mut found = None;
    for header in headers.get_all(header::COOKIE) {
        let source = header.to_str().ok()?;
        if source.len() > 4096 {
            return None;
        }
        for part in source.split(';') {
            if let Some((key, value)) = part.trim().split_once('=')
                && key == name
            {
                if found.is_some()
                    || !(32..=128).contains(&value.len())
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                {
                    return None;
                }
                found = Some(value);
            }
        }
    }
    found
}
fn csrf_valid(state: &AuthState, headers: &HeaderMap, expected: &str) -> bool {
    let origin = state
        .provider
        .config()
        .public_url
        .origin()
        .ascii_serialization();
    headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(origin.as_str())
        && headers
            .get("x-csrf-token")
            .is_some_and(|v| bool::from(v.as_bytes().ct_eq(expected.as_bytes())))
}
async fn login(State(state): State<Arc<AuthState>>) -> Response {
    // A fresh attempt replaces the browser's prior one; cross-site login cannot fixate a session.
    let request = match state.provider.begin_login().await {
        Ok(v) => v,
        Err(_) => return unavailable(),
    };
    let binding = random();
    if state
        .store
        .begin_login(
            &hash(&request.state),
            &hash(&binding),
            &request.verifier,
            &request.nonce,
        )
        .await
        .is_err()
    {
        return unavailable();
    }
    let mut response = Redirect::to(&request.url).into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(&state, true, &binding, 600));
    response
}
#[derive(Deserialize)]
struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}
async fn callback(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
    Query(input): Query<Callback>,
) -> Response {
    let Some(binding) = read_cookie(&state, &headers, true) else {
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
            .finish_login(code, attempt.pkce_verifier, attempt.nonce)
            .await
        {
            Err(_) => error(StatusCode::UNAUTHORIZED, "login_verification_failed"),
            Ok(identity) => {
                let token = random();
                let csrf = random();
                match state
                    .store
                    .create_session(&identity.issuer, &identity.subject, &hash(&token), &csrf)
                    .await
                {
                    Ok(Some(_)) => {
                        // A successful login rotates the session and removes the old local credential.
                        if let Some(old) = read_cookie(&state, &headers, false)
                            && state.store.delete_session(&hash(old)).await.is_err()
                        {
                            return unavailable();
                        }
                        let mut response = Redirect::to("/auth/session").into_response();
                        response
                            .headers_mut()
                            .append(header::SET_COOKIE, cookie(&state, false, &token, 1800));
                        response
                    }
                    Ok(None) => error(StatusCode::FORBIDDEN, "identity_not_linked"),
                    Err(_) => unavailable(),
                }
            }
        }
    } else {
        error(StatusCode::BAD_REQUEST, "invalid_login")
    };
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(&state, true, "", 0));
    response
}
async fn session(State(state): State<Arc<AuthState>>, headers: HeaderMap) -> Response {
    match browser_session(&state, &headers).await {
        Ok(v) => {
            Json(json!({"principal_id":v.principal_id,"csrf_token":v.csrf_token})).into_response()
        }
        Err(v) => v,
    }
}
async fn browser_session(
    state: &AuthState,
    headers: &HeaderMap,
) -> Result<super::BrowserSession, Response> {
    let token = read_cookie(state, headers, false)
        .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "login_required"))?;
    state
        .store
        .session(&hash(token))
        .await
        .map_err(|_| unavailable())?
        .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "login_required"))
}
async fn logout(State(state): State<Arc<AuthState>>, headers: HeaderMap) -> Response {
    let session = match browser_session(&state, &headers).await {
        Ok(v) => v,
        Err(v) => return v,
    };
    if !csrf_valid(&state, &headers, &session.csrf_token) {
        return error(StatusCode::FORBIDDEN, "invalid_csrf");
    }
    let token = read_cookie(&state, &headers, false).expect("session required a cookie");
    if state.store.delete_session(&hash(token)).await.is_err() {
        return unavailable();
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(&state, false, "", 0));
    response
}
async fn browser_auth(
    State(state): State<Arc<AuthState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let session = match browser_session(&state, request.headers()).await {
        Ok(v) => v,
        Err(v) => return v,
    };
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && !csrf_valid(&state, request.headers(), &session.csrf_token)
    {
        return error(StatusCode::FORBIDDEN, "invalid_csrf");
    }
    request.extensions_mut().insert(Caller {
        principal_id: session.principal_id,
        can_write: true,
        workspace_id: None,
    });
    next.run(request).await
}
async fn metadata(State(state): State<Arc<AuthState>>) -> Json<serde_json::Value> {
    Json(
        json!({"resource":state.provider.config().resource(),"authorization_servers":[state.provider.config().issuer_string()],
        "bearer_methods_supported":["header"]}),
    )
}
async fn bearer_auth(
    State(state): State<Arc<AuthState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let expected_host =
        &state.provider.config().public_url[url::Position::BeforeHost..url::Position::AfterPort];
    if request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        != Some(expected_host)
        || request.headers().get(header::ORIGIN).is_some_and(|v| {
            v.to_str().ok()
                != Some(
                    state
                        .provider
                        .config()
                        .public_url
                        .origin()
                        .ascii_serialization()
                        .as_str(),
                )
        })
    {
        return error(StatusCode::FORBIDDEN, "invalid_origin");
    }
    let challenge = || {
        let mut response = error(StatusCode::UNAUTHORIZED, "invalid_bearer_token");
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_str(&format!(
                "Bearer resource_metadata=\"{}\"",
                state.provider.config().metadata_url()
            ))
            .expect("configured URL is header-safe"),
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
    let verified = match state.provider.verify_bearer(token).await {
        Ok(v) => v,
        Err(_) => return challenge(),
    };
    let principal = match state
        .store
        .resolve_identity(&verified.identity.issuer, &verified.identity.subject)
        .await
    {
        Ok(Some(v)) => v,
        Ok(None) => return error(StatusCode::FORBIDDEN, "identity_not_linked"),
        Err(_) => return unavailable(),
    };
    request.extensions_mut().insert(Caller {
        principal_id: principal,
        can_write: verified.can_write,
        workspace_id: None,
    });
    next.run(request).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    after: Option<Uuid>,
    #[serde(default = "page_limit")]
    limit: u16,
}
fn page_limit() -> u16 {
    50
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update<T> {
    expected_version: i32,
    data: T,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    expected_version: i32,
}
fn result<T: serde::Serialize>(value: Result<T, WorkspaceError>) -> Response {
    match value {
        Ok(v) => Json(v).into_response(),
        Err(WorkspaceError::Denied) => error(StatusCode::FORBIDDEN, "access_denied"),
        Err(WorkspaceError::NotFound) => error(StatusCode::NOT_FOUND, "not_found"),
        Err(WorkspaceError::Conflict) => error(StatusCode::CONFLICT, "revision_conflict"),
        Err(WorkspaceError::InvalidInput(_)) => error(StatusCode::BAD_REQUEST, "invalid_input"),
        Err(WorkspaceError::Database(_)) => {
            error(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable")
        }
    }
}
async fn workspace(
    State(s): State<crate::workspaces::WorkspaceStore>,
    axum::Extension(c): axum::Extension<Caller>,
    Path(w): Path<Uuid>,
) -> Response {
    result(s.for_caller(c).workspace(c.principal_id, w).await)
}
macro_rules! configuration_routes {
    ($list:ident,$create:ident,$update:ident,$delete:ident,$input:ty,$list_op:ident,$create_op:ident,$update_op:ident,$delete_op:ident) => {
        async fn $list(
            State(s): State<crate::workspaces::WorkspaceStore>,
            axum::Extension(c): axum::Extension<Caller>,
            Path(w): Path<Uuid>,
            Query(p): Query<Page>,
        ) -> Response {
            result(
                s.for_caller(c)
                    .$list_op(c.principal_id, w, p.after, p.limit)
                    .await,
            )
        }
        async fn $create(
            State(s): State<crate::workspaces::WorkspaceStore>,
            axum::Extension(c): axum::Extension<Caller>,
            Path(w): Path<Uuid>,
            Json(v): Json<$input>,
        ) -> Response {
            result(s.for_caller(c).$create_op(c.principal_id, w, v).await)
        }
        async fn $update(
            State(s): State<crate::workspaces::WorkspaceStore>,
            axum::Extension(c): axum::Extension<Caller>,
            Path((w, id)): Path<(Uuid, Uuid)>,
            Json(v): Json<Update<$input>>,
        ) -> Response {
            result(
                s.for_caller(c)
                    .$update_op(c.principal_id, w, id, v.expected_version, v.data)
                    .await,
            )
        }
        async fn $delete(
            State(s): State<crate::workspaces::WorkspaceStore>,
            axum::Extension(c): axum::Extension<Caller>,
            Path((w, id)): Path<(Uuid, Uuid)>,
            Query(v): Query<Revision>,
        ) -> Response {
            result(
                s.for_caller(c)
                    .$delete_op(c.principal_id, w, id, v.expected_version)
                    .await,
            )
        }
    };
}
configuration_routes!(
    projects,
    create_project,
    update_project,
    delete_project,
    NewProject,
    list_projects,
    create_project,
    update_project,
    delete_project
);
configuration_routes!(
    goals,
    create_goal,
    update_goal,
    delete_goal,
    NewGoal,
    list_goals,
    create_goal,
    update_goal,
    delete_goal
);
configuration_routes!(
    recipients,
    create_recipient,
    update_recipient,
    delete_recipient,
    NewRecipient,
    list_recipients,
    create_recipient,
    update_recipient,
    delete_recipient
);
