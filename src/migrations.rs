use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection};
use sqlx::{ConnectOptions, Connection, Postgres, Transaction};
use thiserror::Error;

use crate::database::POSTGRES_MAJOR;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Migration {
    pub version: String,
    pub checksum: String,
    pub sql: String,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MigrationError {
    #[error("No SQL migrations found")]
    NoMigrationsFound,
    #[error("Migrations must have consecutive numbered filenames")]
    InvalidFilenames,
    #[error("Unable to read migration files")]
    ReadFailed,
    #[error("Unexpected PostgreSQL major version")]
    UnexpectedPostgresMajor,
    #[error("Migration history differs from this release")]
    HistoryDrift,
    #[error("Database migration failed")]
    Database { sqlstate: Option<String> },
}

pub fn load_migrations() -> Result<Vec<Migration>, MigrationError> {
    load_migrations_from(&migration_directory())
}

fn migration_directory() -> PathBuf {
    if let Some(directory) = env::var_os("MIGRATIONS_DIR") {
        return PathBuf::from(directory);
    }

    let container_directory = PathBuf::from("/app/migrations");
    if container_directory.is_dir() {
        return container_directory;
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

fn load_migrations_from(directory: &Path) -> Result<Vec<Migration>, MigrationError> {
    let entries = fs::read_dir(directory).map_err(|_| MigrationError::ReadFailed)?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| MigrationError::ReadFailed)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".sql") {
            names.push(name);
        }
    }
    names.sort();
    validate_migration_versions(names.iter().map(String::as_str))?;

    if names.is_empty() {
        return Err(MigrationError::NoMigrationsFound);
    }

    let mut migrations = Vec::with_capacity(names.len());
    for version in names {
        let sql =
            fs::read_to_string(directory.join(&version)).map_err(|_| MigrationError::ReadFailed)?;
        let checksum = sha256_hex(sql.as_bytes());
        migrations.push(Migration {
            version,
            checksum,
            sql,
        });
    }

    Ok(migrations)
}

fn validate_migration_versions<'a>(
    versions: impl IntoIterator<Item = &'a str>,
) -> Result<(), MigrationError> {
    for (index, version) in versions.into_iter().enumerate() {
        if !valid_migration_name(version) {
            return Err(MigrationError::InvalidFilenames);
        }
        let number = version[..6]
            .parse::<usize>()
            .map_err(|_| MigrationError::InvalidFilenames)?;
        if number != index + 1 {
            return Err(MigrationError::InvalidFilenames);
        }
    }
    Ok(())
}

fn valid_migration_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".sql") else {
        return false;
    };
    let Some((number, description)) = stem.split_once('_') else {
        return false;
    };

    number.len() == 6
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && !description.is_empty()
        && description
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub fn matches_history(
    rows: &[(String, String)],
    migrations: &[Migration],
    complete: bool,
) -> bool {
    (!complete || rows.len() == migrations.len())
        && rows.len() <= migrations.len()
        && rows.iter().enumerate().all(|(index, row)| {
            migrations
                .get(index)
                .is_some_and(|migration| row.0 == migration.version && row.1 == migration.checksum)
        })
}

pub async fn migrate(
    options: PgConnectOptions,
    migrations: &[Migration],
) -> Result<usize, MigrationError> {
    validate_migration_versions(
        migrations
            .iter()
            .map(|migration| migration.version.as_str()),
    )?;

    let options = options
        .application_name("analytics-migrator")
        .options([
            ("statement_timeout", "30000"),
            ("lock_timeout", "10000"),
            ("idle_in_transaction_session_timeout", "30000"),
        ])
        .disable_statement_logging();
    let mut connection =
        tokio::time::timeout(Duration::from_secs(5), PgConnection::connect_with(&options))
            .await
            .map_err(|_| MigrationError::Database { sqlstate: None })?
            .map_err(database_error)?;

    let server = sqlx::query!("SELECT current_setting('server_version_num')::int / 10000 AS major")
        .fetch_one(&mut connection)
        .await
        .map_err(database_error)?;
    if server.major != Some(POSTGRES_MAJOR) {
        return Err(MigrationError::UnexpectedPostgresMajor);
    }

    let mut transaction = connection.begin().await.map_err(database_error)?;
    let result = apply_pending(&mut transaction, migrations).await;
    match result {
        Ok(applied) => {
            transaction.commit().await.map_err(database_error)?;
            Ok(applied)
        }
        Err(error) => {
            let _ = transaction.rollback().await;
            Err(error)
        }
    }
}

async fn apply_pending(
    transaction: &mut Transaction<'_, Postgres>,
    migrations: &[Migration],
) -> Result<usize, MigrationError> {
    sqlx::query!("SELECT pg_advisory_xact_lock(13752787, 1)")
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;

    sqlx::query!(
        r#"
        CREATE TABLE IF NOT EXISTS analytics.schema_migrations (
          version text PRIMARY KEY,
          checksum text NOT NULL,
          applied_at timestamptz NOT NULL DEFAULT now()
        )
        "#
    )
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;

    let history =
        sqlx::query!("SELECT version, checksum FROM analytics.schema_migrations ORDER BY version")
            .fetch_all(&mut **transaction)
            .await
            .map_err(database_error)?;
    let history = history
        .iter()
        .map(|row| (row.version.clone(), row.checksum.clone()))
        .collect::<Vec<_>>();
    if !matches_history(&history, migrations, false) {
        return Err(MigrationError::HistoryDrift);
    }

    let pending = &migrations[history.len()..];
    for migration in pending {
        // Versioned migration files are trusted repository SQL and run only
        // inside this transaction.
        sqlx::raw_sql(sqlx::AssertSqlSafe(migration.sql.as_str()))
            .execute(&mut **transaction)
            .await
            .map_err(database_error)?;
        sqlx::query!(
            "INSERT INTO analytics.schema_migrations (version, checksum) VALUES ($1, $2)",
            migration.version.as_str(),
            migration.checksum.as_str()
        )
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    }

    Ok(pending.len())
}

fn sha256_hex(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing into a string cannot fail");
    }
    output
}

fn database_error(error: sqlx::Error) -> MigrationError {
    let sqlstate = error
        .as_database_error()
        .and_then(|database| database.code())
        .filter(|code| {
            code.len() == 5
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        })
        .map(|code| code.into_owned());
    MigrationError::Database { sqlstate }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let directory =
            env::temp_dir().join(format!("analytics-{label}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn migration_loader_validates_names_and_hashes_exact_file_text() {
        let directory = temporary_directory("migrations");
        let sql = "SELECT 1;\n";
        fs::write(directory.join("000001_foundation.sql"), sql).unwrap();

        let migrations = load_migrations_from(&directory).unwrap();
        assert_eq!(migrations.len(), 1);
        assert_eq!(migrations[0].version, "000001_foundation.sql");
        assert_eq!(migrations[0].sql, sql);
        assert_eq!(migrations[0].checksum, sha256_hex(sql.as_bytes()));

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn migration_loader_rejects_bad_or_incomplete_numbering() {
        let directory = temporary_directory("bad-migrations");
        fs::write(directory.join("1_foundation.sql"), "SELECT 1;").unwrap();
        assert_eq!(
            load_migrations_from(&directory).unwrap_err(),
            MigrationError::InvalidFilenames
        );
        fs::remove_file(directory.join("1_foundation.sql")).unwrap();

        fs::write(directory.join("000002_events.sql"), "SELECT 1;").unwrap();
        assert_eq!(
            load_migrations_from(&directory).unwrap_err(),
            MigrationError::InvalidFilenames
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn migration_history_detects_drift_and_requires_completeness_for_readiness() {
        let migrations = vec![
            Migration {
                version: "000001_foundation.sql".to_owned(),
                checksum: "a".repeat(64),
                sql: String::new(),
            },
            Migration {
                version: "000002_events.sql".to_owned(),
                checksum: "b".repeat(64),
                sql: String::new(),
            },
        ];
        let first_applied = vec![(
            migrations[0].version.clone(),
            migrations[0].checksum.clone(),
        )];

        assert!(matches_history(&first_applied, &migrations, false));
        assert!(!matches_history(&first_applied, &migrations, true));
        assert!(!matches_history(
            &[(migrations[0].version.clone(), "c".repeat(64))],
            &migrations,
            false
        ));
        assert!(matches_history(
            &[
                (
                    migrations[0].version.clone(),
                    migrations[0].checksum.clone()
                ),
                (
                    migrations[1].version.clone(),
                    migrations[1].checksum.clone()
                ),
            ],
            &migrations,
            true
        ));
        assert!(!matches_history(&[], &migrations, true));
    }
}
