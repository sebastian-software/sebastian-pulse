use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, Error, PgPool};

use crate::migrations::Migration;

pub const POSTGRES_MAJOR: i32 = 18;

pub fn create_pool(options: PgConnectOptions) -> PgPool {
    let options = options
        .application_name("sebastian-pulse")
        .options([
            ("statement_timeout", "2000"),
            ("idle_in_transaction_session_timeout", "5000"),
        ])
        .disable_statement_logging();

    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(2))
        .idle_timeout(Some(Duration::from_secs(30)))
        .max_lifetime(Some(Duration::from_secs(300)))
        .connect_lazy_with(options)
}

pub async fn check_readiness(pool: &PgPool, migrations: &[Migration]) -> Result<bool, sqlx::Error> {
    let rows = tokio::time::timeout(
        Duration::from_secs(3),
        sqlx::query!(
            r#"
            SELECT m.version, m.checksum,
              NOT r.rolsuper AND NOT r.rolbypassrls
                AND NOT has_schema_privilege(current_user, 'analytics', 'CREATE')
                AND NOT has_table_privilege(
                  current_user,
                  'analytics.schema_migrations',
                  'INSERT,UPDATE,DELETE,TRUNCATE'
                ) AS safe_role,
              current_setting('server_version_num')::int / 10000 AS major
            FROM analytics.schema_migrations m CROSS JOIN pg_roles r
            WHERE r.rolname = current_user
            ORDER BY m.version
            "#,
        )
        .fetch_all(pool),
    )
    .await
    .map_err(|_| Error::PoolTimedOut)??;

    let history = rows
        .iter()
        .map(|row| (row.version.clone(), row.checksum.clone()))
        .collect::<Vec<_>>();
    Ok(rows
        .iter()
        .all(|row| row.safe_role == Some(true) && row.major == Some(POSTGRES_MAJOR))
        && crate::migrations::matches_history(&history, migrations, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations::Migration;

    fn migration(version: &str, checksum: &str) -> Migration {
        Migration {
            version: version.to_owned(),
            checksum: checksum.to_owned(),
            sql: String::new(),
        }
    }

    #[tokio::test]
    async fn pool_limits_are_fixed_by_the_runtime_contract() {
        let options = PgConnectOptions::new()
            .host("127.0.0.1")
            .database("analytics")
            .username("analytics_app");
        let pool = create_pool(options);
        assert_eq!(pool.options().get_max_connections(), 5);
        assert_eq!(pool.options().get_acquire_timeout(), Duration::from_secs(2));
        assert_eq!(
            pool.options().get_idle_timeout(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            pool.options().get_max_lifetime(),
            Some(Duration::from_secs(300))
        );
        pool.close().await;
    }

    #[test]
    fn readiness_history_must_be_complete_and_exact() {
        let migrations = [
            migration("000001_foundation.sql", "a".repeat(64).as_str()),
            migration("000002_events.sql", "b".repeat(64).as_str()),
        ];
        assert!(!crate::migrations::matches_history(
            &[(
                migrations[0].version.clone(),
                migrations[0].checksum.clone(),
            )],
            &migrations,
            true
        ));
        assert!(!crate::migrations::matches_history(
            &[
                (
                    migrations[0].version.clone(),
                    migrations[0].checksum.clone()
                ),
                (migrations[1].version.clone(), "c".repeat(64)),
            ],
            &migrations,
            true
        ));
    }
}
