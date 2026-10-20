use sqlx::{
    AssertSqlSafe, Connection, PgConnection, Row,
    postgres::{PgConnectOptions, PgSslMode},
};
use std::{
    collections::HashSet,
    future::Future,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

const ADMIN_URL_ENV: &str = "TEST_DATABASE_ADMIN_URL";
pub const APP_ROLE: &str = "analytics_app";
const MIGRATOR_ROLE: &str = "analytics_migrator";
const RUNTIME_ROLE: &str = "analytics_runtime";
const APP_PASSWORD: &str = "local-runtime-only";
const MIGRATOR_PASSWORD: &str = "local-migration-only";
static DATABASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn admin_options() -> PgConnectOptions {
    let source = std::env::var(ADMIN_URL_ENV).unwrap_or_else(|_| {
        panic!("{ADMIN_URL_ENV} is required; use a disposable local PostgreSQL server")
    });
    assert!(
        is_loopback_postgres_admin_url(&source),
        "{ADMIN_URL_ENV} must be a loopback PostgreSQL administrator URL ending in /postgres"
    );
    PgConnectOptions::from_str(&source)
        .unwrap_or_else(|_| panic!("{ADMIN_URL_ENV} must be a valid PostgreSQL URL"))
        .ssl_mode(PgSslMode::Disable)
}

pub fn is_loopback_postgres_admin_url(source: &str) -> bool {
    let Ok(url) = Url::parse(source) else {
        return false;
    };
    matches!(url.scheme(), "postgres" | "postgresql")
        && url.path() == "/postgres"
        && matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
        )
        && url.query().is_none()
}

fn database_name() -> String {
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after UNIX epoch")
        .as_nanos();
    let sequence = DATABASE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "analytics_test_{}_{}_{}",
        std::process::id(),
        tick,
        sequence
    )
}

pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub struct DbFixture {
    pub admin_options: PgConnectOptions,
    pub admin: PgConnection,
    pub owner: PgConnection,
    pub name: String,
    pub migration: PgConnectOptions,
    pub runtime: PgConnectOptions,
}

impl DbFixture {
    async fn create() -> Self {
        let admin_options = admin_options();
        let mut admin = PgConnection::connect_with(&admin_options)
            .await
            .expect("connect to the disposable PostgreSQL administrator database");
        let role_rows = sqlx::query("SELECT rolname FROM pg_roles WHERE rolname = ANY($1)")
            .bind(vec![
                APP_ROLE.to_owned(),
                MIGRATOR_ROLE.to_owned(),
                RUNTIME_ROLE.to_owned(),
            ])
            .fetch_all(&mut admin)
            .await
            .expect("read the shared PostgreSQL test roles");
        let present: HashSet<String> = role_rows
            .iter()
            .map(|row| row.get::<String, _>("rolname"))
            .collect();
        let missing: Vec<&str> = [APP_ROLE, MIGRATOR_ROLE, RUNTIME_ROLE]
            .into_iter()
            .filter(|role| !present.contains(*role))
            .collect();
        assert!(
            missing.is_empty(),
            "the disposable PostgreSQL server must have the shared bootstrap roles already provisioned; missing: {}",
            missing.join(", ")
        );

        let name = database_name();
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {}", ident(&name))))
            .execute(&mut admin)
            .await
            .expect("create a unique disposable database");

        let owner_options = admin_options.clone().database(&name);
        let mut owner = PgConnection::connect_with(&owner_options)
            .await
            .expect("connect to the new disposable database as its administrator");
        sqlx::query(AssertSqlSafe(format!(
            "REVOKE ALL ON DATABASE {} FROM PUBLIC",
            ident(&name)
        )))
        .execute(&mut owner)
        .await
        .expect("revoke public database privileges");
        sqlx::query(AssertSqlSafe(format!(
            "GRANT CONNECT ON DATABASE {} TO {MIGRATOR_ROLE}, {APP_ROLE}",
            ident(&name)
        )))
        .execute(&mut owner)
        .await
        .expect("grant the shared test roles database access");
        sqlx::query("REVOKE ALL ON SCHEMA public FROM PUBLIC")
            .execute(&mut owner)
            .await
            .expect("revoke public schema privileges");
        sqlx::query("CREATE SCHEMA analytics AUTHORIZATION analytics_migrator")
            .execute(&mut owner)
            .await
            .expect("create the analytics schema owned by the shared migrator role");

        let migration = admin_options
            .clone()
            .username(MIGRATOR_ROLE)
            .password(MIGRATOR_PASSWORD)
            .database(&name);
        let runtime = admin_options
            .clone()
            .username(APP_ROLE)
            .password(APP_PASSWORD)
            .database(&name);

        Self {
            admin_options,
            admin,
            owner,
            name,
            migration,
            runtime,
        }
    }
}

pub async fn with_fixture<F, Fut>(body: F)
where
    F: FnOnce(DbFixture) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let fixture = DbFixture::create().await;
    let cleanup_options = fixture.admin_options.clone();
    let name = fixture.name.clone();
    let task = tokio::spawn(body(fixture));
    let test_result = task.await;

    let cleanup_result = async {
        let mut admin = PgConnection::connect_with(&cleanup_options)
            .await
            .expect("reconnect to the disposable PostgreSQL administrator database for cleanup");
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {} WITH (FORCE)",
            ident(&name)
        )))
        .execute(&mut admin)
        .await
        .expect("drop the disposable test database");
        admin.close().await.expect("close the cleanup connection");
    }
    .await;

    match test_result {
        Ok(()) => cleanup_result,
        Err(join_error) => {
            if join_error.is_panic() {
                std::panic::resume_unwind(join_error.into_panic());
            }
            panic!("the PostgreSQL integration test task failed: {join_error}");
        }
    }
}
