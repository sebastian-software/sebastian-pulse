pub mod config;
mod http;
pub mod oidc;
mod store;

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
}

#[derive(Clone)]
pub struct AuthService {
    state: Arc<AuthState>,
}
struct AuthState {
    provider: oidc::OidcProvider,
    store: AuthStore,
    workspaces: WorkspaceStore,
}
impl AuthService {
    pub fn new(pool: PgPool, config: AuthConfig) -> Result<Self, oidc::ProviderError> {
        Ok(Self {
            state: Arc::new(AuthState {
                provider: oidc::OidcProvider::new(config)?,
                store: AuthStore::new(pool.clone()),
                workspaces: WorkspaceStore::new(pool),
            }),
        })
    }
    pub fn start_cleanup(&self) -> tokio::task::JoinHandle<()> {
        let store = self.state.store.clone();
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
        http::router(self.state.clone())
    }
}
