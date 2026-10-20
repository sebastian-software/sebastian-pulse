use sqlx::PgPool;
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

const MAX_LOGIN_ATTEMPTS: i64 = 1_000;
const LOGIN_ATTEMPT_LOCK_NAMESPACE: i32 = 1_597_438_532;
const LOGIN_ATTEMPT_LOCK_ID: i32 = 1;

#[derive(Clone)]
pub struct AuthStore {
    pool: PgPool,
}

#[derive(Clone, Eq, PartialEq)]
pub struct LoginAttempt {
    pub pkce_verifier: String,
    pub nonce: String,
}

impl fmt::Debug for LoginAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoginAttempt")
            .field("pkce_verifier", &"[redacted]")
            .field("nonce", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct BrowserSession {
    pub principal_id: Uuid,
    pub csrf_token: String,
}

impl fmt::Debug for BrowserSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserSession")
            .field("principal_id", &self.principal_id)
            .field("csrf_token", &"[redacted]")
            .finish()
    }
}

#[derive(Error)]
pub enum AuthStoreError {
    #[error("authentication storage is unavailable")]
    Database(#[from] sqlx::Error),
    #[error("too many pending browser login attempts")]
    Capacity,
    #[error("authentication input is invalid")]
    InvalidInput,
}

impl fmt::Debug for AuthStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(_) => formatter.write_str("Database([redacted])"),
            Self::Capacity => formatter.write_str("Capacity"),
            Self::InvalidInput => formatter.write_str("InvalidInput"),
        }
    }
}

impl AuthStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Resolve product IDs only after current managed permission verification.
    /// Organization bindings are operator-managed and never inferred from email.
    pub async fn workos_caller(
        &self,
        client_id: &str,
        access: super::workos::Access,
    ) -> Result<Option<super::Caller>, AuthStoreError> {
        let workspace: Option<Uuid> =
            sqlx::query_scalar("SELECT analytics.resolve_workos_workspace($1, $2)")
                .bind(client_id)
                .bind(&access.organization_id)
                .fetch_one(&self.pool)
                .await?;
        let Some(workspace_id) = workspace else {
            return Ok(None);
        };
        let principal: Option<Uuid> =
            sqlx::query_scalar("SELECT analytics.resolve_workos_principal($1, $2, $3)")
                .bind(client_id)
                .bind(&access.user_id)
                .bind(Uuid::new_v4())
                .fetch_one(&self.pool)
                .await?;
        Ok(principal.map(|principal_id| super::Caller {
            principal_id,
            can_write: access.can_write,
            workspace_id: Some(workspace_id),
        }))
    }

    pub async fn begin_login(
        &self,
        state_hash: &[u8; 32],
        binding_hash: &[u8; 32],
        pkce_verifier: &str,
        nonce: &str,
    ) -> Result<(), AuthStoreError> {
        if !bounded_text(pkce_verifier, 43, 128) || !bounded_text(nonce, 1, 256) {
            return Err(AuthStoreError::InvalidInput);
        }

        let mut tx = self.pool.begin().await?;

        // Serialize admission and cleanup so concurrent starts cannot exceed
        // the global cap between the count and insert.
        sqlx::query!(
            "SELECT pg_catalog.pg_advisory_xact_lock($1, $2)",
            LOGIN_ATTEMPT_LOCK_NAMESPACE,
            LOGIN_ATTEMPT_LOCK_ID
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM analytics.browser_login_attempts WHERE expires_at <= clock_timestamp()"
        )
        .execute(&mut *tx)
        .await?;

        let count = sqlx::query_scalar!(
            "SELECT count(*) FROM analytics.browser_login_attempts WHERE expires_at > clock_timestamp()"
        )
        .fetch_one(&mut *tx)
        .await?;
        if count.unwrap_or(MAX_LOGIN_ATTEMPTS) >= MAX_LOGIN_ATTEMPTS {
            tx.commit().await?;
            return Err(AuthStoreError::Capacity);
        }

        sqlx::query!(
            r#"
            INSERT INTO analytics.browser_login_attempts
                (state_hash, binding_hash, pkce_verifier, nonce, expires_at)
            VALUES ($1, $2, $3, $4, clock_timestamp() + interval '10 minutes')
            "#,
            &state_hash[..],
            &binding_hash[..],
            pkce_verifier,
            nonce
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn consume_login(
        &self,
        state_hash: &[u8; 32],
        binding_hash: &[u8; 32],
    ) -> Result<Option<LoginAttempt>, AuthStoreError> {
        let attempt = sqlx::query_as!(
            LoginAttempt,
            r#"
            DELETE FROM analytics.browser_login_attempts
            WHERE state_hash = $1
              AND binding_hash = $2
              AND expires_at > clock_timestamp()
            RETURNING pkce_verifier, nonce
            "#,
            &state_hash[..],
            &binding_hash[..]
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(attempt)
    }

    pub async fn resolve_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<Uuid>, AuthStoreError> {
        validate_identity(issuer, subject)?;

        let row = sqlx::query!(
            "SELECT analytics.resolve_principal($1, $2) AS principal_id",
            issuer,
            subject
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row.principal_id)
    }

    pub async fn create_session(
        &self,
        issuer: &str,
        subject: &str,
        token_hash: &[u8; 32],
        csrf_token: &str,
    ) -> Result<Option<Uuid>, AuthStoreError> {
        validate_identity(issuer, subject)?;
        if !bounded_text(csrf_token, 43, 128) {
            return Err(AuthStoreError::InvalidInput);
        }

        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "DELETE FROM analytics.browser_sessions WHERE expires_at <= clock_timestamp()"
        )
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query!(
            r#"
            INSERT INTO analytics.browser_sessions
                (token_hash, principal_id, issuer, subject, csrf_token, expires_at)
            SELECT $3, identity.principal_id, $1, $2, $4,
                   clock_timestamp() + interval '30 minutes'
            FROM (SELECT analytics.resolve_principal($1, $2) AS principal_id) AS identity
            WHERE identity.principal_id IS NOT NULL
            RETURNING principal_id
            "#,
            issuer,
            subject,
            &token_hash[..],
            csrf_token
        )
        .fetch_optional(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(row.map(|session| session.principal_id))
    }

    pub async fn session(
        &self,
        token_hash: &[u8; 32],
    ) -> Result<Option<BrowserSession>, AuthStoreError> {
        let mut tx = self.pool.begin().await?;

        // A lookup for an expired bearer token also removes that row.
        sqlx::query!(
            r#"
            DELETE FROM analytics.browser_sessions
            WHERE token_hash = $1 AND expires_at <= clock_timestamp()
            "#,
            &token_hash[..]
        )
        .execute(&mut *tx)
        .await?;

        let session = sqlx::query_as!(
            BrowserSession,
            r#"
            SELECT stored.principal_id, stored.csrf_token
            FROM analytics.browser_sessions AS stored
            WHERE stored.token_hash = $1
              AND stored.expires_at > clock_timestamp()
              AND analytics.resolve_principal(stored.issuer, stored.subject)
                  = stored.principal_id
            "#,
            &token_hash[..]
        )
        .fetch_optional(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(session)
    }

    /// Stored deadlines remain authoritative even while the process is stopped.
    pub async fn cleanup_expired(&self) -> Result<(), AuthStoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "DELETE FROM analytics.browser_login_attempts WHERE expires_at <= clock_timestamp()"
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM analytics.browser_sessions WHERE expires_at <= clock_timestamp()"
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_session(&self, token_hash: &[u8; 32]) -> Result<(), AuthStoreError> {
        sqlx::query!(
            "DELETE FROM analytics.browser_sessions WHERE token_hash = $1",
            &token_hash[..]
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn validate_identity(issuer: &str, subject: &str) -> Result<(), AuthStoreError> {
    if bounded_text(issuer, 1, 2_048) && bounded_text(subject, 1, 255) {
        Ok(())
    } else {
        Err(AuthStoreError::InvalidInput)
    }
}

fn bounded_text(value: &str, minimum: usize, maximum: usize) -> bool {
    !value.contains('\0') && (minimum..=maximum).contains(&value.chars().count())
}
