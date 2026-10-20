//! WorkOS-native boundary for AuthKit sessions and Connect authorization.
//!
//! WorkOS owns users, organizations, memberships, roles, and permission mappings.
//! These types deliberately do not expose provider SDK models to product operations.

use crate::config::Environment;
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use url::Url;
use workos::{
    AuthKitAuthorizationUrlParams, AuthKitPkceCodeExchangeParams, SecretString, SessionData,
    SessionManager,
    organization_membership::ListOrganizationMembershipsParams,
    transport::{HttpRequest, HttpResponse, HttpTransport, TransportError},
};

pub const READ_PERMISSION: &str = "pulse:read";
pub const WRITE_PERMISSION: &str = "pulse:write";
const API_ORIGIN: &str = "https://api.workos.com";
const MAX_RESPONSE_BYTES: usize = 262_144;

/// Redacted error boundary: SDK errors can contain response bodies and tokens.
#[derive(Debug, thiserror::Error)]
pub enum WorkosError {
    #[error("WorkOS configuration is invalid or incomplete")]
    Configuration,
    #[error("WorkOS authentication is invalid or expired")]
    Authentication,
    #[error("WorkOS access is denied")]
    Denied,
    #[error("WorkOS is unavailable")]
    Unavailable,
}

#[derive(Clone)]
pub struct WorkosConfig {
    pub public_url: Url,
    pub connect_issuer: Url,
    pub client_id: String,
    api_key: SecretString,
    cookie_password: SecretString,
}

impl std::fmt::Debug for WorkosConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkosConfig")
            .field("public_url", &self.public_url)
            .field("connect_issuer", &self.connect_issuer)
            .field("credentials", &"<redacted>")
            .finish()
    }
}

impl WorkosConfig {
    /// All settings are required together. Never fall back to another auth mode.
    pub fn from_env(env: &Environment) -> Result<Option<Self>, WorkosError> {
        let keys = [
            "WORKOS_API_KEY",
            "WORKOS_CLIENT_ID",
            "WORKOS_AUTHKIT_URL",
            "WORKOS_COOKIE_PASSWORD",
        ];
        if keys.iter().all(|k| !env.contains_key(*k)) {
            return Ok(None);
        }
        if ["AUTH_ISSUER_URL", "AUTH_CLIENT_ID", "AUTH_CLIENT_SECRET"]
            .iter()
            .any(|k| env.contains_key(*k))
        {
            return Err(WorkosError::Configuration);
        }
        let value = |k: &str| {
            env.get(k)
                .filter(|v| {
                    !v.is_empty() && v.trim() == *v && !v.bytes().any(|b| b.is_ascii_control())
                })
                .ok_or(WorkosError::Configuration)
        };
        let local = matches!(
            env.get("APP_ENV").map(String::as_str),
            Some("local" | "test")
        );
        let public_url = origin(value("AUTH_PUBLIC_URL")?, local)?;
        let connect_issuer = origin(value(keys[2])?, false)?;
        // This proof intentionally uses WorkOS's hosted domain, without a custom domain.
        if !connect_issuer
            .host_str()
            .is_some_and(|h| h.ends_with(".authkit.app"))
            || connect_issuer.port().is_some()
        {
            return Err(WorkosError::Configuration);
        }
        let client_id = value(keys[1])?.clone();
        let api_key = value(keys[0])?;
        let cookie_password = value(keys[3])?;
        if !client_id.starts_with("client_")
            || client_id.len() > 128
            || !client_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !api_key.starts_with("sk_")
            || api_key.len() > 4096
            || cookie_password.len() != 64
            || !cookie_password.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(WorkosError::Configuration);
        }
        Ok(Some(Self {
            public_url,
            connect_issuer,
            client_id,
            api_key: api_key.clone().into(),
            cookie_password: cookie_password.clone().into(),
        }))
    }

    pub fn resource(&self) -> String {
        self.public_url
            .join("mcp")
            .expect("validated origin")
            .to_string()
    }
    pub fn callback(&self) -> String {
        self.public_url
            .join("auth/callback")
            .expect("validated origin")
            .to_string()
    }
    pub fn issuer(&self) -> &str {
        self.connect_issuer.as_str().trim_end_matches('/')
    }
}

fn origin(value: &str, local: bool) -> Result<Url, WorkosError> {
    if value.len() > 2048 {
        return Err(WorkosError::Configuration);
    }
    let url = Url::parse(value).map_err(|_| WorkosError::Configuration)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.host_str().is_none()
        || !(url.scheme() == "https" || (local && loopback && url.scheme() == "http"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(WorkosError::Configuration);
    }
    Ok(url)
}

/// Keep HTTP behavior under the application's timeout, redirect, and size limits.
/// The official SDK supplies API paths, serialization, and typed responses.
struct Transport {
    http: reqwest::Client,
}
#[async_trait]
impl HttpTransport for Transport {
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, TransportError> {
        let url = Url::parse(&req.url).map_err(|_| TransportError::other("invalid WorkOS URL"))?;
        if url.origin().ascii_serialization() != API_ORIGIN
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(TransportError::other("unexpected WorkOS origin"));
        }
        let mut request = self.http.request(req.method, url).headers(req.headers);
        if let Some(body) = req.body {
            request = request.body(body);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| TransportError::other("WorkOS request failed"))?;
        let status = response.status();
        let headers = response.headers().clone();
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
        {
            return Err(TransportError::other("WorkOS response too large"));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| TransportError::other("WorkOS response failed"))?
        {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(TransportError::other("WorkOS response too large"));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse {
            status,
            headers,
            body: body.into(),
        })
    }
}

/// Secrets are intentionally absent from Debug implementations and product output.
pub struct LoginRequest {
    pub url: String,
    pub state: String,
    pub verifier: SecretString,
}
pub struct LoginResult {
    pub user_id: String,
    pub sealed_session: SecretString,
}
pub struct BrowserIdentity {
    pub user_id: String,
    pub organization_id: String,
    pub session_id: String,
    pub renewed_session: Option<SecretString>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub user_id: String,
    pub organization_id: String,
    pub can_write: bool,
}

#[derive(Default)]
struct KeyCache {
    keys: Option<(JwkSet, Instant)>,
    last_attempt: Option<Instant>,
}

pub struct WorkosProvider {
    config: WorkosConfig,
    client: workos::Client,
    http: reqwest::Client,
    keys: Mutex<KeyCache>,
}

impl WorkosProvider {
    #[cfg(feature = "integration-tests")]
    pub fn for_protocol_fixture(
        config: WorkosConfig,
        transport: Arc<dyn HttpTransport>,
        keys: JwkSet,
    ) -> Result<Self, WorkosError> {
        let client = workos::Client::builder()
            .api_key(config.api_key.expose())
            .client_id(&config.client_id)
            .transport(transport)
            .max_retries(0)
            .try_build()
            .map_err(|_| WorkosError::Configuration)?;
        let mut provider = Self::new(config)?;
        provider.client = client;
        provider.keys = Mutex::new(KeyCache {
            keys: Some((keys, Instant::now())),
            last_attempt: None,
        });
        Ok(provider)
    }
    pub fn new(config: WorkosConfig) -> Result<Self, WorkosError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(4))
            .build()
            .map_err(|_| WorkosError::Configuration)?;
        let client = workos::Client::builder()
            .api_key(config.api_key.expose())
            .client_id(&config.client_id)
            .transport(Arc::new(Transport { http: http.clone() }))
            .max_retries(0)
            .try_build()
            .map_err(|_| WorkosError::Configuration)?;
        Ok(Self {
            config,
            client,
            http,
            keys: Mutex::new(KeyCache::default()),
        })
    }
    pub fn config(&self) -> &WorkosConfig {
        &self.config
    }
    pub fn begin_login(&self) -> Result<LoginRequest, WorkosError> {
        let request = self
            .client
            .authkit()
            .pkce_authorization_url(AuthKitAuthorizationUrlParams {
                redirect_uri: self.config.callback(),
                provider: Some("authkit".into()),
                ..Default::default()
            })
            .map_err(|_| WorkosError::Configuration)?;
        Ok(LoginRequest {
            url: request.url,
            state: request.state,
            verifier: request.code_verifier.into(),
        })
    }
    /// The caller must consume one-use, browser-bound state before exchanging this code.
    pub async fn finish_login(
        &self,
        code: String,
        verifier: SecretString,
    ) -> Result<LoginResult, WorkosError> {
        let auth = self
            .client
            .authkit()
            .pkce_code_exchange(AuthKitPkceCodeExchangeParams {
                code,
                code_verifier: verifier.expose().into(),
            })
            .await
            .map_err(api_error)?;
        if !auth.user.email_verified
            || !valid_id(&auth.user.id, "user_")
            || auth.impersonator.is_some()
        {
            return Err(WorkosError::Authentication);
        }
        let data = SessionData {
            access_token: auth.access_token,
            refresh_token: auth.refresh_token,
            user: Some(auth.user.clone()),
            impersonator: None,
        };
        let sealed = workos::seal_session(&data, self.config.cookie_password.expose())
            .map_err(|_| WorkosError::Authentication)?;
        // SDK session authentication trusts a locally sealed token. This cookie is
        // sealed only from the authenticated WorkOS API, never from client input.
        let checked = self.browser_identity(&sealed).await?;
        Ok(LoginResult {
            user_id: auth.user.id,
            sealed_session: checked.renewed_session.unwrap_or_else(|| sealed.into()),
        })
    }
    pub async fn browser_identity(&self, sealed: &str) -> Result<BrowserIdentity, WorkosError> {
        if sealed.len() > 16_384 {
            return Err(WorkosError::Authentication);
        }
        let manager = SessionManager::new(
            Some(&self.client),
            sealed,
            self.config.cookie_password.expose(),
        );
        let mut session = manager.authenticate();
        let mut renewed_session = None;
        let original = session_claims(sealed, self.config.cookie_password.expose())?;
        if session.needs_refresh || original.exp <= jsonwebtoken::get_current_timestamp() {
            // SDK 3.4's sealed-session refresh helper hides the HTTP error and
            // only recognizes invalid_grant at 401. Use its typed API method
            // so documented 400 invalid_grant and transient failures differ.
            let data = workos::unseal_session(sealed, self.config.cookie_password.expose())
                .map_err(|_| WorkosError::Authentication)?;
            let mut params = workos::user_management::AuthenticateWithRefreshTokenParams::new(
                data.refresh_token,
            );
            params.organization_id = (!original.org_id.is_empty()).then(|| original.org_id.clone());
            let auth = self
                .client
                .user_management()
                .authenticate_with_refresh_token(params)
                .await
                .map_err(refresh_error)?;
            let renewed = workos::seal_session(
                &SessionData {
                    access_token: auth.access_token,
                    refresh_token: auth.refresh_token,
                    user: Some(auth.user),
                    impersonator: auth.impersonator,
                },
                self.config.cookie_password.expose(),
            )
            .map_err(|_| WorkosError::Authentication)?;
            session = SessionManager::new(
                Some(&self.client),
                &renewed,
                self.config.cookie_password.expose(),
            )
            .authenticate();
            renewed_session = Some(renewed.into());
        }
        let user = session.user.ok_or(WorkosError::Authentication)?;
        let current = session_claims(
            renewed_session
                .as_ref()
                .map_or(sealed, SecretString::expose),
            self.config.cookie_password.expose(),
        )?;
        let now = jsonwebtoken::get_current_timestamp();
        if !session.authenticated
            || !user.email_verified
            || !valid_id(&user.id, "user_")
            || !valid_id(&session.session_id, "session_")
            || session.impersonator.is_some()
            || current.iss.trim_end_matches('/') != API_ORIGIN
            || current.sub != user.id
            || original.sub != current.sub
            || original.sid != current.sid
            || current.sid != session.session_id
            || current.org_id != session.organization_id
            || current.exp <= now
            || current.exp <= current.iat
            || current.iat > now.saturating_add(30)
        {
            return Err(WorkosError::Authentication);
        }
        Ok(BrowserIdentity {
            user_id: user.id,
            organization_id: session.organization_id,
            session_id: session.session_id,
            renewed_session,
        })
    }
    pub async fn revoke_session(&self, session_id: &str) -> Result<(), WorkosError> {
        if !valid_id(session_id, "session_") {
            return Err(WorkosError::Authentication);
        }
        self.client
            .user_management()
            .revoke_session(workos::user_management::RevokeSessionParams::new(
                workos::RevokeSession {
                    session_id: session_id.into(),
                },
            ))
            .await
            .map_err(api_error)
    }
    /// Resolve current WorkOS memberships and role permissions. No local grants or
    /// stale permission cache can authorize access when WorkOS is unavailable.
    pub async fn permissions(&self, user: &str, organization: &str) -> Result<Access, WorkosError> {
        if !valid_id(user, "user_") || !valid_id(organization, "org_") {
            return Err(WorkosError::Denied);
        }
        let page = self
            .client
            .organization_membership()
            .list_organization_memberships(ListOrganizationMembershipsParams {
                user_id: Some(user.into()),
                organization_id: Some(organization.into()),
                limit: Some(2),
                ..Default::default()
            })
            .await
            .map_err(api_error)?;
        if page.data.len() != 1 {
            return Err(WorkosError::Denied);
        }
        let membership = &page.data[0];
        if membership.user_id != user
            || membership.organization_id != organization
            || membership.status.as_ref() != "active"
        {
            return Err(WorkosError::Denied);
        }
        let mut slugs = BTreeSet::new();
        slugs.insert(membership.role.slug.as_str());
        slugs.extend(membership.roles.iter().map(|r| r.slug.as_str()));
        if slugs.len() > 16 {
            return Err(WorkosError::Denied);
        }
        let mut permissions = BTreeSet::new();
        for slug in slugs {
            let role = self
                .client
                .authorization()
                .get_organization_role(organization, slug)
                .await
                .map_err(api_error)?;
            if role.slug != slug {
                return Err(WorkosError::Denied);
            }
            permissions.extend(role.permissions);
        }
        access(user, organization, &permissions, true)
    }
    /// Connect client delegation and the user's current WorkOS permissions must
    /// both allow the operation. An AuthKit session JWT is not an MCP bearer.
    pub async fn verify_bearer(&self, token: &str) -> Result<Access, WorkosError> {
        let claims = self.connect_claims(token).await?;
        let scopes: BTreeSet<_> = claims.scope.split_ascii_whitespace().collect();
        if !scopes.contains(READ_PERMISSION) {
            return Err(WorkosError::Denied);
        }
        let mut granted = self.permissions(&claims.sub, &claims.org_id).await?;
        granted.can_write &= scopes.contains(WRITE_PERMISSION);
        Ok(granted)
    }
    async fn connect_claims(&self, token: &str) -> Result<ConnectClaims, WorkosError> {
        if token.len() > 16_384 {
            return Err(WorkosError::Authentication);
        }
        let header = decode_header(token).map_err(|_| WorkosError::Authentication)?;
        if header.alg != Algorithm::RS256 || header.jku.is_some() || header.jwk.is_some() {
            return Err(WorkosError::Authentication);
        }
        let kid = header.kid.ok_or(WorkosError::Authentication)?;
        let mut cache = self.keys.lock().await;
        let fresh = cache
            .keys
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(300));
        let known = cache
            .keys
            .as_ref()
            .is_some_and(|(keys, _)| keys.find(&kid).is_some());
        if !fresh || !known {
            if cache
                .last_attempt
                .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
            {
                return Err(WorkosError::Authentication);
            }
            cache.last_attempt = Some(Instant::now());
            let mut response = self
                .http
                .get(
                    self.config
                        .connect_issuer
                        .join("oauth2/jwks")
                        .expect("validated origin"),
                )
                .send()
                .await
                .map_err(|_| WorkosError::Unavailable)?;
            if !response.status().is_success()
                || response
                    .content_length()
                    .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
            {
                return Err(WorkosError::Unavailable);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| WorkosError::Unavailable)?
            {
                if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    return Err(WorkosError::Unavailable);
                }
                body.extend_from_slice(&chunk);
            }
            let keys: JwkSet =
                serde_json::from_slice(&body).map_err(|_| WorkosError::Unavailable)?;
            if keys.keys.is_empty() || keys.keys.len() > 32 {
                return Err(WorkosError::Unavailable);
            }
            cache.keys = Some((keys, Instant::now()));
        }
        let key = cache
            .keys
            .as_ref()
            .and_then(|(keys, _)| keys.find(&kid))
            .ok_or(WorkosError::Authentication)?;
        let key = DecodingKey::from_jwk(key).map_err(|_| WorkosError::Authentication)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.config.issuer()]);
        validation.set_audience(&[self.config.resource()]);
        validation.set_required_spec_claims(&["exp", "iat", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 0;
        let claims = decode::<ConnectClaims>(token, &key, &validation)
            .map_err(|_| WorkosError::Authentication)?
            .claims;
        let now = jsonwebtoken::get_current_timestamp();
        if !valid_id(&claims.sub, "user_")
            || !valid_id(&claims.org_id, "org_")
            || claims.exp <= now
            || claims.exp <= claims.iat
            || claims.iat > now.saturating_add(30)
        {
            return Err(WorkosError::Authentication);
        }
        Ok(claims)
    }
}

#[derive(Deserialize)]
struct SessionClaims {
    iss: String,
    sub: String,
    sid: String,
    #[serde(default)]
    org_id: String,
    exp: u64,
    iat: u64,
}

// Only parse after authenticated decryption: tokens were sealed from WorkOS's
// authenticated API response. This is not verification of an arbitrary JWT.
fn session_claims(sealed: &str, password: &str) -> Result<SessionClaims, WorkosError> {
    let data = workos::unseal_session(sealed, password).map_err(|_| WorkosError::Authentication)?;
    let payload = data
        .access_token
        .expose()
        .split('.')
        .nth(1)
        .ok_or(WorkosError::Authentication)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| WorkosError::Authentication)?;
    serde_json::from_slice(&bytes).map_err(|_| WorkosError::Authentication)
}

#[derive(Deserialize)]
struct ConnectClaims {
    sub: String,
    org_id: String,
    scope: String,
    exp: u64,
    iat: u64,
}
fn valid_id(value: &str, prefix: &str) -> bool {
    value.starts_with(prefix)
        && value.len() > prefix.len()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn access(
    user: &str,
    organization: &str,
    permissions: &BTreeSet<String>,
    client_write: bool,
) -> Result<Access, WorkosError> {
    if !permissions.contains(READ_PERMISSION) {
        return Err(WorkosError::Denied);
    }
    Ok(Access {
        user_id: user.into(),
        organization_id: organization.into(),
        can_write: client_write && permissions.contains(WRITE_PERMISSION),
    })
}
fn api_error(error: workos::Error) -> WorkosError {
    if error.is_unauthorized() {
        WorkosError::Authentication
    } else if error.is_not_found() {
        WorkosError::Denied
    } else {
        WorkosError::Unavailable
    }
}

fn refresh_error(error: workos::Error) -> WorkosError {
    match error.status() {
        Some(status) if (400..500).contains(&status) && ![408, 429].contains(&status) => {
            WorkosError::Authentication
        }
        _ => WorkosError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde_json::{Value, json};
    use std::{collections::VecDeque, sync::Mutex as StdMutex};

    #[derive(Default)]
    struct Fixture {
        responses: StdMutex<VecDeque<(u16, Value)>>,
        requests: StdMutex<Vec<HttpRequest>>,
    }
    impl Fixture {
        fn respond(&self, status: u16, body: Value) {
            self.responses.lock().unwrap().push_back((status, body));
        }
    }
    #[async_trait]
    impl HttpTransport for Fixture {
        async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.requests.lock().unwrap().push(req);
            let (status, body) = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected WorkOS API call");
            Ok(HttpResponse {
                status: axum::http::StatusCode::from_u16(status).unwrap(),
                headers: Default::default(),
                body: serde_json::to_vec(&body).unwrap().into(),
            })
        }
    }
    fn env() -> Environment {
        Environment::from([
            ("WORKOS_API_KEY".into(), "sk_fixture_not_a_real_key".into()),
            ("WORKOS_CLIENT_ID".into(), "client_fixture".into()),
            (
                "WORKOS_AUTHKIT_URL".into(),
                "https://fixture.authkit.app".into(),
            ),
            ("WORKOS_COOKIE_PASSWORD".into(), "ab".repeat(32)),
            (
                "AUTH_PUBLIC_URL".into(),
                "https://pulse.example.invalid".into(),
            ),
        ])
    }
    fn provider() -> (WorkosProvider, Arc<Fixture>) {
        let mut provider =
            WorkosProvider::new(WorkosConfig::from_env(&env()).unwrap().unwrap()).unwrap();
        let fixture = Arc::new(Fixture::default());
        provider.client = workos::Client::builder()
            .api_key("sk_fixture_not_a_real_key")
            .client_id("client_fixture")
            .transport(fixture.clone())
            .max_retries(0)
            .try_build()
            .unwrap();
        (provider, fixture)
    }
    fn user() -> Value {
        json!({"object":"user","id":"user_fixture","email":"fixture@example.invalid","email_verified":true,"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z"})
    }
    fn membership(status: &str, role: &str, roles: &[&str]) -> Value {
        json!({"object":"organization_membership","id":"om_fixture","user_id":"user_fixture","organization_id":"org_fixture","status":status,"directory_managed":false,"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z","role":{"slug":role},"roles":roles.iter().map(|slug|json!({"slug":slug})).collect::<Vec<_>>(),"user":user()})
    }
    fn role(slug: &str, permissions: &[&str]) -> Value {
        json!({"object":"role","id":"role_fixture","slug":slug,"name":"Fixture","type":"EnvironmentRole","resource_type_slug":"organization","permissions":permissions,"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z"})
    }
    fn queue_permissions(fixture: &Fixture, permissions: &[&str]) {
        fixture.respond(200, json!({"object":"list","data":[membership("active","member",&[])],"list_metadata":{"before":null,"after":null}}));
        fixture.respond(200, role("member", permissions));
    }
    fn token(claims: Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("initial".into());
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(include_bytes!(
                "../../tests/fixtures/oidc-insecure-test-key.pem"
            ))
            .unwrap(),
        )
        .unwrap()
    }
    fn connect_token(scope: &str) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        token(
            json!({"iss":"https://fixture.authkit.app","aud":"https://pulse.example.invalid/mcp","sub":"user_fixture","org_id":"org_fixture","scope":scope,"iat":now,"exp":now+300}),
        )
    }
    async fn cached_keys(provider: &WorkosProvider) {
        provider.keys.lock().await.keys = Some((
            serde_json::from_str(include_str!("../../tests/fixtures/oidc-jwks.json")).unwrap(),
            Instant::now(),
        ));
    }

    #[test]
    fn configuration_is_atomic_redacted_and_uses_the_hosted_domain() {
        assert!(
            WorkosConfig::from_env(&Environment::new())
                .unwrap()
                .is_none()
        );
        for key in [
            "WORKOS_API_KEY",
            "WORKOS_CLIENT_ID",
            "WORKOS_AUTHKIT_URL",
            "WORKOS_COOKIE_PASSWORD",
            "AUTH_PUBLIC_URL",
        ] {
            let mut values = env();
            values.remove(key);
            assert!(WorkosConfig::from_env(&values).is_err(), "missing {key}");
        }
        for issuer in [
            "http://fixture.authkit.app",
            "https://authkit.app.evil.invalid",
            "https://fixture.authkit.app/path",
            "https://fixture.authkit.app?query=1",
        ] {
            let mut values = env();
            values.insert("WORKOS_AUTHKIT_URL".into(), issuer.into());
            assert!(WorkosConfig::from_env(&values).is_err());
        }
        let mut values = env();
        values.insert("AUTH_CLIENT_SECRET".into(), "legacy".into());
        assert!(WorkosConfig::from_env(&values).is_err());
        let config = WorkosConfig::from_env(&env()).unwrap().unwrap();
        assert!(!format!("{config:?}").contains("sk_fixture"));
        assert!(!format!("{config:?}").contains(&"ab".repeat(32)));
    }
    #[test]
    fn authkit_login_uses_unique_state_and_s256_without_disclosing_the_api_key() {
        let (provider, _) = provider();
        let first = provider.begin_login().unwrap();
        let second = provider.begin_login().unwrap();
        assert_ne!(first.state, second.state);
        let url = Url::parse(&first.url).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(url.origin().ascii_serialization(), API_ORIGIN);
        assert_eq!(query["provider"], "authkit");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["redirect_uri"], provider.config.callback());
        assert_eq!(query["state"], first.state);
        assert!(!first.url.contains("sk_fixture"));
        assert!(!first.url.contains(first.verifier.expose()));
    }
    #[tokio::test]
    async fn workos_membership_and_permission_mapping_are_authoritative() {
        let (provider, fixture) = provider();
        queue_permissions(&fixture, &[READ_PERMISSION, WRITE_PERMISSION]);
        assert!(
            provider
                .permissions("user_fixture", "org_fixture")
                .await
                .unwrap()
                .can_write
        );
        queue_permissions(&fixture, &[READ_PERMISSION]);
        assert!(
            !provider
                .permissions("user_fixture", "org_fixture")
                .await
                .unwrap()
                .can_write
        );
        queue_permissions(&fixture, &[WRITE_PERMISSION]);
        assert!(matches!(
            provider.permissions("user_fixture", "org_fixture").await,
            Err(WorkosError::Denied)
        ));
        fixture.respond(
            200,
            json!({"object":"list","data":[],"list_metadata":{"before":null,"after":null}}),
        );
        assert!(matches!(
            provider.permissions("user_fixture", "org_fixture").await,
            Err(WorkosError::Denied)
        ));
        fixture.respond(503, json!({"message":"fixture unavailable"}));
        assert!(matches!(
            provider.permissions("user_fixture", "org_fixture").await,
            Err(WorkosError::Unavailable)
        ));
        let requests = fixture.requests.lock().unwrap();
        let query = Url::parse(&requests[0].url).unwrap();
        assert!(
            query
                .query_pairs()
                .any(|(k, v)| k == "user_id" && v == "user_fixture")
        );
        assert!(
            query
                .query_pairs()
                .any(|(k, v)| k == "organization_id" && v == "org_fixture")
        );
        assert!(
            requests[1]
                .url
                .ends_with("/authorization/organizations/org_fixture/roles/member")
        );
    }
    #[tokio::test]
    async fn inactive_and_mismatched_memberships_cannot_grant_access() {
        let (provider, fixture) = provider();
        for mut data in [
            membership("inactive", "admin", &[]),
            membership("pending", "admin", &[]),
            membership("active", "admin", &[]),
        ] {
            if data["status"] == "active" {
                data["organization_id"] = json!("org_other");
            }
            fixture.respond(
                200,
                json!({"object":"list","data":[data],"list_metadata":{"before":null,"after":null}}),
            );
            assert!(matches!(
                provider.permissions("user_fixture", "org_fixture").await,
                Err(WorkosError::Denied)
            ));
        }
    }
    #[tokio::test]
    async fn mcp_scopes_intersect_with_current_user_permissions() {
        let (provider, fixture) = provider();
        cached_keys(&provider).await;
        queue_permissions(&fixture, &[READ_PERMISSION, WRITE_PERMISSION]);
        assert!(
            !provider
                .verify_bearer(&connect_token(READ_PERMISSION))
                .await
                .unwrap()
                .can_write
        );
        queue_permissions(&fixture, &[READ_PERMISSION]);
        assert!(
            !provider
                .verify_bearer(&connect_token("pulse:read pulse:write"))
                .await
                .unwrap()
                .can_write
        );
        queue_permissions(&fixture, &[READ_PERMISSION, WRITE_PERMISSION]);
        assert!(
            provider
                .verify_bearer(&connect_token("pulse:read pulse:write"))
                .await
                .unwrap()
                .can_write
        );
        assert!(
            provider
                .verify_bearer(&connect_token(WRITE_PERMISSION))
                .await
                .is_err()
        );
        let count = fixture.requests.lock().unwrap().len();
        let now = jsonwebtoken::get_current_timestamp();
        for claims in [
            json!({"iss":"https://other.authkit.app","aud":provider.config.resource(),"sub":"user_fixture","org_id":"org_fixture","scope":READ_PERMISSION,"iat":now,"exp":now+300}),
            json!({"iss":provider.config.issuer(),"aud":"client_fixture","sub":"user_fixture","org_id":"org_fixture","scope":READ_PERMISSION,"iat":now,"exp":now+300}),
            json!({"iss":provider.config.issuer(),"aud":provider.config.resource(),"sub":"client_machine","org_id":"org_fixture","scope":READ_PERMISSION,"iat":now,"exp":now+300}),
            json!({"iss":provider.config.issuer(),"aud":provider.config.resource(),"sub":"user_fixture","org_id":"org_fixture","scope":READ_PERMISSION,"iat":now-600,"exp":now-1}),
        ] {
            assert!(provider.verify_bearer(&token(claims)).await.is_err());
        }
        assert_eq!(
            fixture.requests.lock().unwrap().len(),
            count,
            "invalid tokens must not query WorkOS permissions"
        );
    }
    #[tokio::test]
    async fn native_login_refresh_and_revocation_use_the_official_sdk() {
        let (provider, fixture) = provider();
        let now = jsonwebtoken::get_current_timestamp();
        let session = token(
            json!({"iss":API_ORIGIN,"sub":"user_fixture","sid":"session_fixture","org_id":"org_fixture","iat":now,"exp":now+300}),
        );
        fixture.respond(
            200,
            json!({"user":user(),"access_token":session,"refresh_token":"refresh_fixture"}),
        );
        let logged_in = provider
            .finish_login("code_fixture".into(), "verifier_fixture".into())
            .await
            .unwrap();
        assert_eq!(logged_in.user_id, "user_fixture");
        let identity = provider
            .browser_identity(logged_in.sealed_session.expose())
            .await
            .unwrap();
        assert_eq!(identity.organization_id, "org_fixture");
        let expired = workos::seal_session(
            &SessionData {
                access_token: token(
                    json!({"iss":API_ORIGIN,"sub":"user_fixture","sid":"session_fixture","org_id":"org_fixture","iat":now-300,"exp":now-10}),
                )
                .into(),
                refresh_token: "refresh_fixture".into(),
                user: Some(serde_json::from_value(user()).unwrap()),
                impersonator: None,
            },
            provider.config.cookie_password.expose(),
        )
        .unwrap();
        fixture.respond(
            200,
            json!({"user":user(),"access_token":session,"refresh_token":"refresh_rotated"}),
        );
        let refreshed = provider.browser_identity(&expired).await.unwrap();
        assert!(refreshed.renewed_session.is_some());
        let mut tampered = logged_in.sealed_session.expose().as_bytes().to_vec();
        tampered[25] ^= 1;
        assert!(
            provider
                .browser_identity(std::str::from_utf8(&tampered).unwrap())
                .await
                .is_err()
        );
        fixture.respond(204, json!(null));
        provider.revoke_session("session_fixture").await.unwrap();
        let requests = fixture.requests.lock().unwrap();
        let code: Value = serde_json::from_slice(requests[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(code["grant_type"], "authorization_code");
        assert_eq!(code["code_verifier"], "verifier_fixture");
        let refresh: Value = serde_json::from_slice(requests[1].body.as_ref().unwrap()).unwrap();
        assert_eq!(refresh["grant_type"], "refresh_token");
        assert_eq!(refresh["organization_id"], "org_fixture");
        assert!(
            requests[2]
                .url
                .ends_with("/user_management/sessions/revoke")
        );
    }

    #[tokio::test]
    async fn transient_refresh_failures_preserve_the_session_and_invalid_grant_is_terminal() {
        let (provider, fixture) = provider();
        let now = jsonwebtoken::get_current_timestamp();
        let expired = workos::seal_session(&SessionData {
            access_token: token(json!({"iss":API_ORIGIN,"sub":"user_fixture","sid":"session_fixture","org_id":"org_fixture","iat":now-300,"exp":now-1})).into(),
            refresh_token: "refresh_fixture".into(),
            user: Some(serde_json::from_value(user()).unwrap()), impersonator: None,
        }, provider.config.cookie_password.expose()).unwrap();
        for status in [429, 503] {
            fixture.respond(status, json!({"message":"temporary failure"}));
            assert!(matches!(
                provider.browser_identity(&expired).await,
                Err(WorkosError::Unavailable)
            ));
        }
        fixture.respond(
            400,
            json!({"code":"invalid_grant","message":"session ended"}),
        );
        assert!(matches!(
            provider.browser_identity(&expired).await,
            Err(WorkosError::Authentication)
        ));
        let requests = fixture.requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            3,
            "each request makes one bounded refresh attempt"
        );
        for request in requests.iter() {
            let body: serde_json::Value =
                serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
            assert_eq!(body["refresh_token"], "refresh_fixture");
        }
    }
}
