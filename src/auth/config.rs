use crate::config::Environment;
use thiserror::Error;
use url::Url;

/// Provider-independent OIDC/OAuth settings. Workspace grants never come from these settings.
#[derive(Clone)]
pub struct AuthConfig {
    pub issuer: Url,
    pub issuer_id: String,
    pub public_url: Url,
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthConfig")
            .field("issuer", &self.issuer)
            .field("public_url", &self.public_url)
            .field("credentials", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error(
    "Authentication requires valid AUTH_ISSUER_URL, AUTH_PUBLIC_URL, AUTH_CLIENT_ID, and AUTH_CLIENT_SECRET"
)]
pub struct AuthConfigurationError;

impl AuthConfig {
    /// No settings disables auth routes. Partial settings fail startup instead of enabling a fallback.
    pub fn from_env(env: &Environment) -> Result<Option<Self>, AuthConfigurationError> {
        let keys = [
            "AUTH_ISSUER_URL",
            "AUTH_PUBLIC_URL",
            "AUTH_CLIENT_ID",
            "AUTH_CLIENT_SECRET",
        ];
        if keys.iter().all(|key| !env.contains_key(*key)) {
            return Ok(None);
        }
        let value = |key| {
            env.get(key)
                .filter(|v| !v.is_empty())
                .ok_or(AuthConfigurationError)
        };
        let local = matches!(
            env.get("APP_ENV").map(String::as_str),
            Some("local" | "test")
        );
        let issuer_id = value(keys[0])?.clone();
        let issuer = parse_url(&issuer_id, local, false)?;
        let public_url = parse_origin(value(keys[1])?, local)?;
        let client_id = value(keys[2])?.clone();
        let client_secret = value(keys[3])?.clone();
        if client_id.len() > 256
            || client_id
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
            || client_secret.len() > 4096
            || client_secret.bytes().any(|b| b.is_ascii_control())
        {
            return Err(AuthConfigurationError);
        }
        Ok(Some(Self {
            issuer,
            issuer_id,
            public_url,
            client_id,
            client_secret,
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
    pub fn metadata_url(&self) -> String {
        self.public_url
            .join(".well-known/oauth-protected-resource/mcp")
            .expect("validated origin")
            .to_string()
    }
    pub fn secure_cookies(&self) -> bool {
        self.public_url.scheme() == "https"
    }
    pub fn issuer_string(&self) -> String {
        self.issuer_id.clone()
    }
}

fn parse_origin(value: &str, local: bool) -> Result<Url, AuthConfigurationError> {
    parse_url(value, local, true)
}

fn parse_url(value: &str, local: bool, origin_only: bool) -> Result<Url, AuthConfigurationError> {
    if value.len() > 2048 || value.trim() != value || value.bytes().any(|b| b.is_ascii_control()) {
        return Err(AuthConfigurationError);
    }
    let url = Url::parse(value).map_err(|_| AuthConfigurationError)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.host_str().is_none()
        || !(url.scheme() == "https" || (local && loopback && url.scheme() == "http"))
        || !url.username().is_empty()
        || url.password().is_some()
        || (origin_only && url.path() != "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AuthConfigurationError);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn env(issuer: &str) -> Environment {
        Environment::from([
            ("AUTH_ISSUER_URL".into(), issuer.into()),
            (
                "AUTH_PUBLIC_URL".into(),
                "https://analytics.example.com".into(),
            ),
            ("AUTH_CLIENT_ID".into(), "client_fixture".into()),
            ("AUTH_CLIENT_SECRET".into(), "fixture-secret".into()),
        ])
    }
    #[test]
    fn disabled_partial_and_unsafe_settings_fail_closed() {
        assert!(AuthConfig::from_env(&Environment::new()).unwrap().is_none());
        assert!(
            AuthConfig::from_env(&Environment::from([("AUTH_CLIENT_ID".into(), "x".into())]))
                .is_err()
        );
        for issuer in [
            "http://example.com",
            "https://u:p@example.com",
            "https://example.com/?x=1",
            "https://example.com/#x",
        ] {
            assert!(AuthConfig::from_env(&env(issuer)).is_err());
        }
        let mut local = env("http://127.0.0.1:12345");
        assert!(AuthConfig::from_env(&local).is_err());
        local.insert("APP_ENV".into(), "test".into());
        assert!(AuthConfig::from_env(&local).is_ok());
    }
}
