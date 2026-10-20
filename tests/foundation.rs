use std::{net::SocketAddr, time::Duration};

use analytics_service::{
    app::Service,
    database::create_pool,
    migrations::{Migration, load_migrations, matches_history, migrate},
};
use axum::Router;
use reqwest::{Client, Response};
use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, Row};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle, time::timeout};

mod support;
use support::*;

#[test]
fn administrator_fixture_cannot_redirect_through_url_parameters() {
    assert!(is_loopback_postgres_admin_url(
        "postgresql://postgres@127.0.0.1:55432/postgres"
    ));
    for source in [
        "postgresql://postgres@db.example/postgres",
        "postgresql://postgres@127.0.0.1/postgres?host=db.example",
        "postgresql://postgres@127.0.0.1/postgres?hostaddr=192.0.2.1",
        "postgresql://postgres@localhost/postgres?dbname=production",
    ] {
        assert!(!is_loopback_postgres_admin_url(source));
    }
}

fn sha256_hex(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn test_migration(version_number: usize, sql: &str) -> Migration {
    Migration {
        version: format!("{version_number:06}_test.sql"),
        checksum: sha256_hex(sql.as_bytes()),
        sql: sql.to_owned(),
    }
}

fn assert_permission_denied(error: sqlx::Error, sql: &str) {
    let code = match error {
        sqlx::Error::Database(error) => error.code().map(|code| code.into_owned()),
        other => panic!("{sql} failed for an unexpected reason: {other}"),
    };
    assert_eq!(
        code.as_deref(),
        Some("42501"),
        "{sql} must be denied by PostgreSQL"
    );
}

struct RunningService {
    address: SocketAddr,
    stop: oneshot::Sender<()>,
    task: JoinHandle<std::io::Result<()>>,
}

async fn serve(router: Router) -> RunningService {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the integration HTTP server to an ephemeral loopback port");
    let address = listener
        .local_addr()
        .expect("read the ephemeral HTTP server address");
    let (stop, stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
    });
    RunningService {
        address,
        stop,
        task,
    }
}

impl RunningService {
    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.address, path)
    }

    async fn close(self) {
        let _ = self.stop.send(());
        self.task
            .await
            .expect("join the HTTP server task")
            .expect("stop the HTTP server cleanly");
    }
}

fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .expect("build the integration HTTP client")
}

async fn get(client: &Client, server: &RunningService, path: &str) -> Response {
    client
        .get(server.url(path))
        .send()
        .await
        .unwrap_or_else(|error| panic!("GET {path} failed: {error}"))
}

async fn wait_for_ready(client: &Client, server: &RunningService) -> Response {
    let mut last = None;
    for _ in 0..10 {
        let response = get(client, server, "/health/ready").await;
        if response.status().as_u16() == 200 {
            return response;
        }
        last = Some(response);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    last.expect("at least one readiness response")
}

async fn assert_ready(client: &Client, server: &RunningService) {
    let response = wait_for_ready(client, server).await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!({"status": "ready", "release": "integration"})
    );
}

#[tokio::test]
async fn migration_locking_idempotency_and_history_rejections_match_the_foundation_contract() {
    with_fixture(|mut db| async move {
        let migrations = load_migrations().expect("load the checked-in immutable SQL migrations");
        assert!(
            !migrations.is_empty(),
            "the foundation migration must be present"
        );
        let migration_count = migrations.len();
        let first = db.migration.clone();
        let second = db.migration.clone();
        let first_migrations = migrations.clone();
        let second_migrations = migrations.clone();
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
        let first_barrier = barrier.clone();
        let first_task = tokio::spawn(async move {
            first_barrier.wait().await;
            migrate(first, &first_migrations).await
        });
        let second_barrier = barrier.clone();
        let second_task = tokio::spawn(async move {
            second_barrier.wait().await;
            migrate(second, &second_migrations).await
        });
        barrier.wait().await;
        let mut counts = vec![
            first_task.await.expect("join first migrator").unwrap(),
            second_task.await.expect("join second migrator").unwrap(),
        ];
        counts.sort_unstable();
        assert_eq!(counts, vec![0, migration_count]);
        assert_eq!(migrate(db.migration.clone(), &migrations).await.unwrap(), 0);

        let rows = sqlx::query(
            "SELECT version, checksum FROM analytics.schema_migrations ORDER BY version",
        )
        .fetch_all(&mut db.owner)
        .await
        .expect("read the original analytics migration ledger");
        let ledger: Vec<(String, String)> = rows
            .iter()
            .map(|row| (row.get("version"), row.get("checksum")))
            .collect();
        assert_eq!(ledger.len(), migration_count);
        assert!(matches_history(&ledger, &migrations, true));

        let changed = migrations
            .iter()
            .cloned()
            .map(|mut migration| {
                migration.checksum = "changed".to_owned();
                migration
            })
            .collect::<Vec<_>>();
        let drift_error = migrate(db.migration.clone(), &changed)
            .await
            .expect_err("reject a changed checksum for applied SQL");
        assert!(drift_error.to_string().contains("history differs"));
        let ahead_error = migrate(db.migration.clone(), &[])
            .await
            .expect_err("reject a release whose migration list is behind the database");
        assert!(ahead_error.to_string().contains("history differs"));

        if migration_count > 1 {
            let partial = ledger[..migration_count - 1].to_vec();
            assert!(matches_history(&partial, &migrations, false));
            assert!(!matches_history(&partial, &migrations, true));
        } else {
            assert!(!matches_history(&[], &migrations, true));
        }
        let future = vec![("999999_future.sql".to_owned(), "future".to_owned())];
        assert!(!matches_history(&future, &migrations, false));
    })
    .await;
}

#[tokio::test]
async fn failed_migrations_roll_back_sql_and_history_including_initial_ledger_creation() {
    with_fixture(|mut db| async move {
        let migrations = load_migrations()
            .expect("load the checked-in immutable SQL migrations");
        let bad_sql = "CREATE TABLE analytics.must_rollback (id integer); SELECT * FROM analytics.this_table_does_not_exist;";
        let mut fresh_failure = migrations.clone();
        fresh_failure.push(test_migration(migrations.len() + 1, bad_sql));
        assert!(
            migrate(db.migration.clone(), &fresh_failure).await.is_err(),
            "the invalid SQL must fail"
        );
        let ledger_exists: Option<String> = sqlx::query_scalar(
            "SELECT to_regclass('analytics.schema_migrations')::text",
        )
        .fetch_one(&mut db.owner)
        .await
        .expect("check whether the failed first transaction left a ledger");
        let rolled_back_table: Option<String> = sqlx::query_scalar(
            "SELECT to_regclass('analytics.must_rollback')::text",
        )
        .fetch_one(&mut db.owner)
        .await
        .expect("check whether the failed migration left its table");
        assert_eq!(ledger_exists, None, "ledger creation must roll back");
        assert_eq!(rolled_back_table, None, "migration DDL must roll back");

        migrate(db.migration.clone(), &migrations)
            .await
            .expect("apply the foundation migration after the failed transaction");
        let mut pending_failure = migrations.clone();
        pending_failure.push(test_migration(migrations.len() + 1, bad_sql));
        assert!(
            migrate(db.migration.clone(), &pending_failure).await.is_err(),
            "a failed pending migration must fail"
        );
        let table: Option<String> = sqlx::query_scalar(
            "SELECT to_regclass('analytics.must_rollback')::text",
        )
        .fetch_one(&mut db.owner)
        .await
        .expect("check that pending DDL rolled back");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.schema_migrations")
            .fetch_one(&mut db.owner)
            .await
            .expect("read the unchanged migration ledger");
        assert_eq!(table, None);
        assert_eq!(count, migrations.len() as i64);
    })
    .await;
}

#[tokio::test]
async fn runtime_role_can_use_application_data_but_cannot_migrate_or_elevate_itself() {
    with_fixture(|db| async move {
        let migrations = load_migrations()
            .expect("load the checked-in immutable SQL migrations");
        let table_sql = "CREATE TABLE analytics.synthetic_probe (id integer PRIMARY KEY, value text NOT NULL); GRANT SELECT, INSERT, UPDATE, DELETE ON analytics.synthetic_probe TO analytics_runtime";
        let mut with_probe = migrations.clone();
        with_probe.push(test_migration(migrations.len() + 1, table_sql));
        migrate(db.migration.clone(), &with_probe)
            .await
            .expect("create the test application table with the migrator");

        let pool = create_pool(db.runtime.clone());
        sqlx::query("INSERT INTO analytics.synthetic_probe VALUES (1, 'before')")
            .execute(&pool)
            .await
            .expect("runtime can insert application data");
        sqlx::query("UPDATE analytics.synthetic_probe SET value = 'after' WHERE id = 1")
            .execute(&pool)
            .await
            .expect("runtime can update application data");
        let value: String = sqlx::query_scalar(
            "SELECT value FROM analytics.synthetic_probe WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("runtime can read application data");
        assert_eq!(value, "after");
        sqlx::query("DELETE FROM analytics.synthetic_probe WHERE id = 1")
            .execute(&pool)
            .await
            .expect("runtime can delete application data");

        for statement in [
            "CREATE TABLE analytics.forbidden (id integer)",
            "ALTER TABLE analytics.synthetic_probe ADD COLUMN forbidden integer",
            "DELETE FROM analytics.schema_migrations",
            "TRUNCATE analytics.schema_migrations",
            "UPDATE analytics.schema_migrations SET checksum = 'forbidden'",
            "INSERT INTO analytics.schema_migrations (version, checksum) VALUES ('999999_forbidden.sql', 'forbidden')",
            "SET ROLE analytics_migrator",
            "CREATE ROLE forbidden_superuser SUPERUSER",
        ] {
            let error = sqlx::query(statement)
                .execute(&pool)
                .await
                .expect_err("the runtime role must not run DDL, write history, or elevate itself");
            assert_permission_denied(error, statement);
        }

        let history_count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.schema_migrations")
            .fetch_one(&pool)
            .await
            .expect("runtime retains read-only access to migration history");
        assert_eq!(history_count, with_probe.len() as i64);

        let mut held = Vec::with_capacity(5);
        for _ in 0..5 {
            held.push(
                timeout(Duration::from_secs(8), pool.acquire())
                    .await
                    .expect("acquire one of the five configured runtime connections")
                    .expect("runtime pool connection must succeed"),
            );
        }
        assert_eq!(pool.size(), 5, "the runtime pool limit remains five");
        assert!(
            timeout(Duration::from_millis(200), pool.acquire())
                .await
                .is_err(),
            "a sixth runtime connection must wait while all five are checked out"
        );
        drop(held);
        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn readiness_tracks_missing_schema_database_outage_drift_recovery_and_drain_over_http() {
    with_fixture(|mut db| async move {
        let migrations = load_migrations()
            .expect("load the checked-in immutable SQL migrations");
        let pool = create_pool(db.runtime.clone());
        let service = Service::new(pool.clone(), migrations.clone(), "integration".to_owned());
        let server = serve(service.router()).await;
        let client = http_client();

        let live = get(&client, &server, "/health/live").await;
        assert_eq!(live.status(), reqwest::StatusCode::OK);
        assert_eq!(live.headers().get("cache-control").unwrap(), "no-store");
        assert_eq!(
            live.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({"status": "live", "release": "integration"})
        );
        let missing = get(&client, &server, "/health/ready").await;
        assert_eq!(missing.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(missing.headers().get("cache-control").unwrap(), "no-store");
        assert_eq!(
            missing.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({"status": "unavailable", "release": "integration"})
        );
        let not_found = get(&client, &server, "/missing").await;
        assert_eq!(not_found.status(), reqwest::StatusCode::NOT_FOUND);
        assert_eq!(not_found.headers().get("cache-control").unwrap(), "no-store");
        assert_eq!(
            not_found.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({"error": "not_found"})
        );

        migrate(db.migration.clone(), &migrations)
            .await
            .expect("apply the original migration history");
        assert_ready(&client, &server).await;

        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {} ALLOW_CONNECTIONS false",
            ident(&db.name)
        )))
        .execute(&mut db.admin)
        .await
        .expect("block new connections to the disposable database");
        sqlx::query(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND usename = $2 AND pid <> pg_backend_pid()",
        )
        .bind(&db.name)
        .bind(APP_ROLE)
        .fetch_all(&mut db.admin)
        .await
        .expect("terminate runtime connections to simulate a database outage");
        let live_during_outage = get(&client, &server, "/health/live").await;
        assert_eq!(live_during_outage.status(), reqwest::StatusCode::OK);
        let blocked = get(&client, &server, "/health/ready").await;
        assert_eq!(blocked.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(blocked.headers().get("cache-control").unwrap(), "no-store");

        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {} ALLOW_CONNECTIONS true",
            ident(&db.name)
        )))
        .execute(&mut db.admin)
        .await
        .expect("restore database connections");
        assert_ready(&client, &server).await;

        sqlx::query("UPDATE analytics.schema_migrations SET checksum = 'drift' WHERE version = $1")
            .bind(&migrations[0].version)
            .execute(&mut db.owner)
            .await
            .expect("simulate migration checksum drift");
        let drifted = get(&client, &server, "/health/ready").await;
        assert_eq!(drifted.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        sqlx::query("UPDATE analytics.schema_migrations SET checksum = $1 WHERE version = $2")
            .bind(&migrations[0].checksum)
            .bind(&migrations[0].version)
            .execute(&mut db.owner)
            .await
            .expect("restore the original migration checksum");
        assert_ready(&client, &server).await;

        let last_migration = migrations.last().expect("at least one migration");
        sqlx::query("DELETE FROM analytics.schema_migrations WHERE version = $1")
            .bind(&last_migration.version)
            .execute(&mut db.owner)
            .await
            .expect("simulate missing migration history");
        let incomplete = get(&client, &server, "/health/ready").await;
        assert_eq!(incomplete.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        sqlx::query("INSERT INTO analytics.schema_migrations (version, checksum) VALUES ($1, $2)")
            .bind(&last_migration.version)
            .bind(&last_migration.checksum)
            .execute(&mut db.owner)
            .await
            .expect("restore the simulated missing ledger row without reapplying DDL");
        assert_ready(&client, &server).await;

        sqlx::query(
            "INSERT INTO analytics.schema_migrations (version, checksum) VALUES ('999999_future.sql', 'future')",
        )
        .execute(&mut db.owner)
        .await
        .expect("simulate a database with a migration from a future release");
        let ahead = get(&client, &server, "/health/ready").await;
        assert_eq!(ahead.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        sqlx::query("DELETE FROM analytics.schema_migrations WHERE version = '999999_future.sql'")
            .execute(&mut db.owner)
            .await
            .expect("remove the synthetic future migration row");
        assert_ready(&client, &server).await;

        service.drain();
        let drained_live = get(&client, &server, "/health/live").await;
        assert_eq!(drained_live.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(drained_live.headers().get("cache-control").unwrap(), "no-store");
        assert_eq!(
            drained_live.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({"status": "draining", "release": "integration"})
        );
        let drained_ready = get(&client, &server, "/health/ready").await;
        assert_eq!(drained_ready.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(drained_ready.headers().get("cache-control").unwrap(), "no-store");

        server.close().await;
        pool.close().await;
    })
    .await;
}

#[tokio::test]
async fn readiness_rejects_the_schema_owner_role_even_with_current_history() {
    with_fixture(|db| async move {
        let migrations = load_migrations().expect("load the checked-in immutable SQL migrations");
        migrate(db.migration.clone(), &migrations)
            .await
            .expect("apply the original migration history");

        let pool = create_pool(db.migration.clone());
        let service = Service::new(pool.clone(), migrations, "integration".to_owned());
        let server = serve(service.router()).await;
        let response = get(&http_client(), &server, "/health/ready").await;
        assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({"status": "unavailable", "release": "integration"})
        );

        server.close().await;
        pool.close().await;
    })
    .await;
}
