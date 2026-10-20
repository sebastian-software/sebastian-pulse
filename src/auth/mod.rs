pub mod config;
mod http;
pub mod oidc;
mod store;
pub mod workos;
mod workos_http;

use crate::workspaces::WorkspaceStore;
use axum::Router;
pub use config::{AuthConfig, AuthConfigurationError};
use sqlx::PgPool;
use std::sync::Arc;
pub use store::{AuthStore, AuthStoreError, BrowserSession, LoginAttempt};

/// Installed by the transport after provider verification and local identity resolution.
/// Never deserialize this type from tool arguments or HTTP input.
#[derive(Clone, Copy)]
pub struct Caller {
    pub principal_id: uuid::Uuid,
    pub can_write: bool,
    /// Some binds managed authority to exactly one workspace; None uses legacy membership.
    pub workspace_id: Option<uuid::Uuid>,
}

#[derive(Clone)]
pub struct AuthService {
    backend: Backend,
}
#[derive(Clone)]
enum Backend {
    Oidc(Arc<AuthState>),
    Workos(Arc<workos_http::WorkosState>),
}

struct AuthState {
    provider: oidc::OidcProvider,
    store: AuthStore,
    workspaces: WorkspaceStore,
}
impl AuthService {
    pub fn new(pool: PgPool, config: AuthConfig) -> Result<Self, oidc::ProviderError> {
        Ok(Self {
            backend: Backend::Oidc(Arc::new(AuthState {
                provider: oidc::OidcProvider::new(config)?,
                store: AuthStore::new(pool.clone()),
                workspaces: WorkspaceStore::new(pool),
            })),
        })
    }
    pub fn new_workos(
        pool: PgPool,
        config: workos::WorkosConfig,
    ) -> Result<Self, workos::WorkosError> {
        Self::with_workos_provider(pool, workos::WorkosProvider::new(config)?)
    }
    pub fn with_workos_provider(
        pool: PgPool,
        provider: workos::WorkosProvider,
    ) -> Result<Self, workos::WorkosError> {
        Ok(Self {
            backend: Backend::Workos(Arc::new(workos_http::WorkosState::new(pool, provider))),
        })
    }
    pub fn start_cleanup(&self) -> tokio::task::JoinHandle<()> {
        let store = match &self.backend {
            Backend::Oidc(s) => s.store.clone(),
            Backend::Workos(s) => s.store.clone(),
        };
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(std::time::Duration::from_secs(60));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                timer.tick().await;
                if store.cleanup_expired().await.is_err() {
                    crate::log::event("auth_state_cleanup_failed", serde_json::json!({}));
                }
            }
        })
    }
    pub fn router(&self) -> Router {
        match &self.backend {
            Backend::Oidc(s) => http::router(s.clone()),
            Backend::Workos(s) => workos_http::router(s.clone()),
        }
    }
}
