//! Standard OIDC adapter; no WorkOS IDs, organization roles, API keys, or SDKs enter the domain.
use super::config::AuthConfig;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, HttpRequest, HttpResponse, IssuerUrl, Nonce, OAuth2TokenResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    core::{
        CoreAuthenticationFlow, CoreClient, CoreJwsSigningAlgorithm, CoreProviderMetadata,
        CoreUserInfoClaims,
    },
};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub const READ_SCOPE: &str = "mcp:read";
pub const WRITE_SCOPE: &str = "mcp:write";
type Client = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

#[derive(Debug, thiserror::Error)]
#[error("Identity provider request or verification failed")]
pub struct ProviderError;

#[derive(Clone)]
pub struct OidcProvider {
    config: AuthConfig,
    http: reqwest::Client,
    snapshot: Arc<Mutex<KeyCache>>,
}
#[derive(Default)]
struct KeyCache {
    current: Option<Snapshot>,
    last_attempt: Option<Instant>,
}
struct Snapshot {
    client: Client,
    keys: JwkSet,
    fetched: Instant,
}

pub struct LoginRequest {
    pub url: String,
    pub state: String,
    pub nonce: String,
    pub verifier: String,
}
pub struct VerifiedIdentity {
    pub issuer: String,
    pub subject: String,
}
pub struct VerifiedBearer {
    pub identity: VerifiedIdentity,
    pub can_write: bool,
}

#[derive(Clone, Deserialize)]
struct AccessClaims {
    iss: String,
    sub: String,
    exp: u64,
    iat: u64,
    scope: String,
}

impl OidcProvider {
    pub fn new(config: AuthConfig) -> Result<Self, ProviderError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(4))
            .connect_timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| ProviderError)?;
        Ok(Self {
            config,
            http,
            snapshot: Arc::new(Mutex::new(KeyCache::default())),
        })
    }

    pub fn config(&self) -> &AuthConfig {
        &self.config
    }

    // All requests, including discovery-advertised JWKS/userinfo/token endpoints, stay on the
    // configured issuer origin. Tokens can never supply a fetch URL or an alternate issuer.
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse, ProviderError> {
        let url = url::Url::parse(&request.uri().to_string()).map_err(|_| ProviderError)?;
        if url.origin() != self.config.issuer.origin()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ProviderError);
        }
        let (parts, body) = request.into_parts();
        let mut response = self
            .http
            .request(parts.method, url)
            .headers(parts.headers)
            .body(body)
            .send()
            .await
            .map_err(|_| ProviderError)?;
        if response
            .content_length()
            .is_some_and(|length| length > 262_144)
        {
            return Err(ProviderError);
        }
        let mut body = Vec::new();
        let mut output = openidconnect::http::Response::builder().status(response.status());
        for (key, value) in response.headers() {
            output = output.header(key, value);
        }
        while let Some(chunk) = response.chunk().await.map_err(|_| ProviderError)? {
            if body.len() + chunk.len() > 262_144 {
                return Err(ProviderError);
            }
            body.extend_from_slice(&chunk);
        }
        output.body(body).map_err(|_| ProviderError)
    }

    async fn discover(&self) -> Result<Snapshot, ProviderError> {
        let metadata = CoreProviderMetadata::discover_async(
            IssuerUrl::new(self.config.issuer_string()).map_err(|_| ProviderError)?,
            self,
        )
        .await
        .map_err(|_| ProviderError)?;
        for endpoint in [
            Some(metadata.authorization_endpoint().url()),
            metadata.token_endpoint().map(|v| v.url()),
            metadata.userinfo_endpoint().map(|v| v.url()),
            Some(metadata.jwks_uri().url()),
        ]
        .into_iter()
        .flatten()
        {
            if endpoint.origin() != self.config.issuer.origin()
                || endpoint.query().is_some()
                || endpoint.fragment().is_some()
            {
                return Err(ProviderError);
            }
        }
        if metadata.token_endpoint().is_none() || metadata.userinfo_endpoint().is_none() {
            return Err(ProviderError);
        }
        let keys: JwkSet = serde_json::from_value(
            serde_json::to_value(metadata.jwks()).map_err(|_| ProviderError)?,
        )
        .map_err(|_| ProviderError)?;
        if keys.keys.is_empty() || keys.keys.len() > 32 {
            return Err(ProviderError);
        }
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
        )
        .set_redirect_uri(RedirectUrl::new(self.config.callback()).map_err(|_| ProviderError)?);
        Ok(Snapshot {
            client,
            keys,
            fetched: Instant::now(),
        })
    }

    async fn refresh(&self, cache: &mut KeyCache) -> Result<(), ProviderError> {
        // Remember failed attempts too: untrusted key IDs cannot amplify an upstream outage.
        if cache
            .last_attempt
            .is_some_and(|time| time.elapsed() < Duration::from_secs(10))
        {
            return Err(ProviderError);
        }
        cache.last_attempt = Some(Instant::now());
        cache.current = Some(self.discover().await?);
        Ok(())
    }

    async fn client(&self) -> Result<Client, ProviderError> {
        let mut snapshot = self.snapshot.lock().await;
        if snapshot
            .current
            .as_ref()
            .is_none_or(|s| s.fetched.elapsed() >= Duration::from_secs(300))
        {
            self.refresh(&mut snapshot).await?;
        }
        Ok(snapshot
            .current
            .as_ref()
            .ok_or(ProviderError)?
            .client
            .clone())
    }

    pub async fn begin_login(&self) -> Result<LoginRequest, ProviderError> {
        let client = self.client().await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("email".into()))
            .set_pkce_challenge(challenge)
            .url();
        Ok(LoginRequest {
            url: url.to_string(),
            state: state.secret().clone(),
            nonce: nonce.secret().clone(),
            verifier: verifier.secret().clone(),
        })
    }

    pub async fn finish_login(
        &self,
        code: String,
        verifier: String,
        nonce: String,
    ) -> Result<VerifiedIdentity, ProviderError> {
        let client = self.client().await?;
        let tokens = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|_| ProviderError)?
            .set_pkce_verifier(PkceCodeVerifier::new(verifier))
            .request_async(self)
            .await
            .map_err(|_| ProviderError)?;
        let id_token = tokens.id_token().ok_or(ProviderError)?;
        // Signing-key rotation also applies to browser ID tokens, not only MCP access tokens.
        let header = decode_header(id_token.to_string()).map_err(|_| ProviderError)?;
        if header.alg != Algorithm::RS256 {
            return Err(ProviderError);
        }
        let kid = header.kid.ok_or(ProviderError)?;
        let client = self.client_for_key(&kid).await?;
        let verifier = client
            .id_token_verifier()
            .set_allowed_algs([CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256]);
        let claims = id_token
            .claims(&verifier, &Nonce::new(nonce))
            .map_err(|_| ProviderError)?;
        if let Some(expected) = claims.access_token_hash() {
            let actual = AccessTokenHash::from_token(
                tokens.access_token(),
                id_token.signing_alg().map_err(|_| ProviderError)?,
                id_token.signing_key(&verifier).map_err(|_| ProviderError)?,
            )
            .map_err(|_| ProviderError)?;
            if actual != *expected {
                return Err(ProviderError);
            }
        }
        // The provider may omit email_verified from its ID token. Userinfo binds the verified
        // email claim to this exact authenticated subject; no email is used as a principal key.
        let userinfo: CoreUserInfoClaims = client
            .user_info(
                tokens.access_token().clone(),
                Some(claims.subject().clone()),
            )
            .map_err(|_| ProviderError)?
            .request_async(self)
            .await
            .map_err(|_| ProviderError)?;
        if userinfo.email_verified() != Some(true) || userinfo.email().is_none() {
            return Err(ProviderError);
        }
        let subject = claims.subject().as_str();
        if subject.is_empty() || subject.len() > 255 {
            return Err(ProviderError);
        }
        Ok(VerifiedIdentity {
            issuer: self.config.issuer_string(),
            subject: subject.to_owned(),
        })
    }

    async fn client_for_key(&self, kid: &str) -> Result<Client, ProviderError> {
        let _ = self.client().await?;
        let mut cache = self.snapshot.lock().await;
        if cache
            .current
            .as_ref()
            .is_none_or(|s| s.keys.find(kid).is_none())
        {
            self.refresh(&mut cache).await?;
        }
        let snapshot = cache.current.as_ref().ok_or(ProviderError)?;
        if snapshot.keys.find(kid).is_none() {
            return Err(ProviderError);
        }
        Ok(snapshot.client.clone())
    }

    pub async fn verify_bearer(&self, token: &str) -> Result<VerifiedBearer, ProviderError> {
        if token.len() > 16_384 {
            return Err(ProviderError);
        }
        let header = decode_header(token).map_err(|_| ProviderError)?;
        if header.alg != Algorithm::RS256 || header.jku.is_some() || header.jwk.is_some() {
            return Err(ProviderError);
        }
        let kid = header.kid.ok_or(ProviderError)?;
        let _ = self.client_for_key(&kid).await?;
        let snapshot = self.snapshot.lock().await;
        let key = snapshot
            .current
            .as_ref()
            .and_then(|s| s.keys.find(&kid))
            .ok_or(ProviderError)?;
        let key = DecodingKey::from_jwk(key).map_err(|_| ProviderError)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.config.issuer_string()]);
        validation.set_audience(&[self.config.resource()]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 0;
        validation.validate_nbf = true;
        let claims = decode::<AccessClaims>(token, &key, &validation)
            .map_err(|_| ProviderError)?
            .claims;
        let now = jsonwebtoken::get_current_timestamp();
        // The JWT library matches the exact resource in either standard aud representation.
        // ID tokens naming only the browser/MCP client ID fail the resource-audience check.
        if claims.sub.is_empty()
            || claims.sub.len() > 255
            || claims.iat > now.saturating_add(30)
            || claims.exp <= now
            || claims.exp <= claims.iat
        {
            return Err(ProviderError);
        }
        let scopes: Vec<_> = claims.scope.split_ascii_whitespace().collect();
        if !scopes.contains(&READ_SCOPE) {
            return Err(ProviderError);
        }
        Ok(VerifiedBearer {
            identity: VerifiedIdentity {
                issuer: claims.iss,
                subject: claims.sub,
            },
            can_write: scopes.contains(&WRITE_SCOPE),
        })
    }
}

impl<'c> openidconnect::AsyncHttpClient<'c> for OidcProvider {
    type Error = ProviderError;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<HttpResponse, ProviderError>> + Send + 'c>,
    >;
    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(self.request(request))
    }
}
