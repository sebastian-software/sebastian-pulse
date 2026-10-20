use sebastian_pulse::{
    auth::{AuthStore, AuthStoreError, BrowserSession, LoginAttempt},
    database::create_pool,
    migrations::{load_migrations, migrate},
};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection, Row};
use uuid::Uuid;

#[allow(dead_code)]
mod support;
use support::*;

async fn prepare(db: &mut DbFixture) -> AuthStore {
    let migrations = load_migrations().expect("load migrations");
    migrate(db.migration.clone(), &migrations)
        .await
        .expect("apply migrations to the disposable database");
    AuthStore::new(create_pool(db.runtime.clone()))
}

async fn create_principal(db: &mut DbFixture) -> Uuid {
    let principal_id = Uuid::new_v4();
    sqlx::query("INSERT INTO analytics.principals (id) VALUES ($1)")
        .bind(principal_id)
        .execute(&mut db.owner)
        .await
        .expect("seed a principal through the trusted connection");
    principal_id
}

async fn link_identity(db: &mut DbFixture, issuer: &str, subject: &str, principal_id: Uuid) {
    sqlx::query(
        "INSERT INTO analytics.identity_links (issuer, subject, principal_id) VALUES ($1, $2, $3)",
    )
    .bind(issuer)
    .bind(subject)
    .bind(principal_id)
    .execute(&mut db.owner)
    .await
    .expect("link an exact provider identity through the trusted connection");
}

#[tokio::test]
async fn unmapped_provider_identities_cannot_resolve_or_create_sessions() {
    with_fixture(|mut db| async move {
        let store = prepare(&mut db).await;
        assert_eq!(
            store
                .resolve_identity("https://login.example.test", "external-user-1")
                .await
                .expect("resolve an unmapped provider identity"),
            None
        );
        assert_eq!(
            store
                .create_session(
                    "https://login.example.test",
                    "external-user-1",
                    &[1; 32],
                    &"c".repeat(43),
                )
                .await
                .expect("try to create a session for an unmapped provider identity"),
            None
        );
        assert_eq!(
            store
                .session(&[1; 32])
                .await
                .expect("look up a missing session"),
            None
        );
    })
    .await;
}

#[tokio::test]
async fn login_attempts_require_binding_are_one_use_and_expire() {
    with_fixture(|mut db| async move {
        let store = prepare(&mut db).await;
        let state_hash = [11; 32];
        let binding_hash = [12; 32];
        let wrong_binding = [13; 32];
        store
            .begin_login(&state_hash, &binding_hash, &"p".repeat(43), "nonce-value")
            .await
            .expect("begin a bounded login attempt");

        assert_eq!(
            store
                .consume_login(&state_hash, &wrong_binding)
                .await
                .expect("try consuming an attempt with the wrong binding"),
            None,
            "a wrong binding must not consume the pending attempt"
        );
        assert_eq!(
            store
                .consume_login(&state_hash, &binding_hash)
                .await
                .expect("consume the attempt with its matching binding"),
            Some(LoginAttempt {
                pkce_verifier: "p".repeat(43),
                nonce: "nonce-value".to_owned(),
            })
        );
        assert_eq!(
            store
                .consume_login(&state_hash, &binding_hash)
                .await
                .expect("replay the consumed attempt"),
            None,
            "a consumed attempt must not be replayable"
        );

        let expired_state_hash = [14; 32];
        let expired_binding_hash = [15; 32];
        store
            .begin_login(
                &expired_state_hash,
                &expired_binding_hash,
                &"q".repeat(43),
                "expired-nonce",
            )
            .await
            .expect("begin an attempt that the trusted connection will expire");
        sqlx::query(
            "UPDATE analytics.browser_login_attempts SET expires_at = now() - interval '1 second' WHERE state_hash = $1",
        )
        .bind(&expired_state_hash[..])
        .execute(&mut db.owner)
        .await
        .expect("expire the attempt through the trusted connection");
        assert_eq!(
            store
                .consume_login(&expired_state_hash, &expired_binding_hash)
                .await
                .expect("try consuming an expired attempt"),
            None
        );
    })
    .await;
}

#[tokio::test]
async fn login_attempt_admission_is_bounded_and_cleans_expired_rows() {
    with_fixture(|mut db| async move {
        let store = prepare(&mut db).await;
        sqlx::query(
            r#"
            INSERT INTO analytics.browser_login_attempts
                (state_hash, binding_hash, pkce_verifier, nonce, expires_at)
            SELECT decode(lpad(to_hex(attempt_id), 64, '0'), 'hex'),
                   decode(repeat('0', 64), 'hex'),
                   repeat('p', 43), 'nonce', now() + interval '5 minutes'
            FROM generate_series(1, 1000) AS series(attempt_id)
            "#,
        )
        .execute(&mut db.owner)
        .await
        .expect("seed the bounded global login attempt capacity");

        assert!(matches!(
            store
                .begin_login(&[21; 32], &[22; 32], &"v".repeat(43), "nonce")
                .await,
            Err(AuthStoreError::Capacity)
        ));

        sqlx::query(
            "UPDATE analytics.browser_login_attempts SET expires_at = now() - interval '1 second'",
        )
        .execute(&mut db.owner)
        .await
        .expect("expire the seeded pending attempts");
        store
            .begin_login(&[23; 32], &[24; 32], &"w".repeat(43), "nonce")
            .await
            .expect("expired rows should be cleaned before admitting a new attempt");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM analytics.browser_login_attempts WHERE expires_at > now()",
            )
            .fetch_one(&mut db.owner)
            .await
            .expect("count the unexpired attempts after cleanup"),
            1
        );
    })
    .await;
}

#[tokio::test]
async fn sessions_store_only_hashes_and_recheck_identity_link_lifecycle() {
    with_fixture(|mut db| async move {
        let store = prepare(&mut db).await;
        let principal_id = create_principal(&mut db).await;
        let issuer = "https://login.example.test";
        let subject = "provider-subject";
        link_identity(&mut db, issuer, subject, principal_id).await;

        let bearer_token = [31; 32];
        let token_hash: [u8; 32] = Sha256::digest(bearer_token).into();
        let csrf_token = "c".repeat(43);
        assert_eq!(
            store
                .create_session(issuer, subject, &token_hash, &csrf_token)
                .await
                .expect("create a session for a mapped identity"),
            Some(principal_id)
        );
        assert_eq!(
            store
                .session(&token_hash)
                .await
                .expect("resolve the hashed bearer token"),
            Some(BrowserSession {
                principal_id,
                csrf_token: csrf_token.clone(),
            })
        );
        let stored_hash = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT token_hash FROM analytics.browser_sessions WHERE token_hash = $1",
        )
        .bind(&token_hash[..])
        .fetch_one(&mut db.owner)
        .await
        .expect("inspect the stored token digest through the trusted connection");
        assert_eq!(stored_hash, token_hash);

        sqlx::query(
            "UPDATE analytics.identity_links SET enabled = false WHERE issuer = $1 AND subject = $2",
        )
        .bind(issuer)
        .bind(subject)
        .execute(&mut db.owner)
        .await
        .expect("disable the identity link through the trusted connection");
        assert_eq!(
            store
                .session(&token_hash)
                .await
                .expect("resolve a session whose provider identity is disabled"),
            None,
            "an existing session must recheck the exact enabled identity link"
        );

        sqlx::query(
            "UPDATE analytics.identity_links SET enabled = true WHERE issuer = $1 AND subject = $2",
        )
        .bind(issuer)
        .bind(subject)
        .execute(&mut db.owner)
        .await
        .expect("re-enable the identity link for the unlink/relink check");
        let relink_token_hash = [32; 32];
        store
            .create_session(issuer, subject, &relink_token_hash, &csrf_token)
            .await
            .expect("create a session before unlinking the provider identity");
        sqlx::query("DELETE FROM analytics.identity_links WHERE issuer = $1 AND subject = $2")
            .bind(issuer)
            .bind(subject)
            .execute(&mut db.owner)
            .await
            .expect("unlink the provider identity through the trusted connection");
        link_identity(&mut db, issuer, subject, principal_id).await;
        assert_eq!(
            store
                .session(&relink_token_hash)
                .await
                .expect("resolve a token created before unlink/relink"),
            None,
            "unlink/relink must not revive an old browser session"
        );

        let expired_hash = [33; 32];
        store
            .create_session(issuer, subject, &expired_hash, &csrf_token)
            .await
            .expect("create a session to expire");
        sqlx::query(
            "UPDATE analytics.browser_sessions SET expires_at = now() - interval '1 second' WHERE token_hash = $1",
        )
        .bind(&expired_hash[..])
        .execute(&mut db.owner)
        .await
        .expect("expire the session through the trusted connection");
        assert_eq!(
            store
                .session(&expired_hash)
                .await
                .expect("resolve an expired session"),
            None
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM analytics.browser_sessions WHERE token_hash = $1",
            )
            .bind(&expired_hash[..])
            .fetch_one(&mut db.owner)
            .await
            .expect("verify that an expired lookup removed its row"),
            0
        );

        store
            .delete_session(&token_hash)
            .await
            .expect("revoke a session locally");
        assert_eq!(
            store
                .session(&token_hash)
                .await
                .expect("resolve a locally revoked session"),
            None
        );
    })
    .await;
}

#[tokio::test]
async fn multiple_oidc_identities_for_one_principal_do_not_change_grants() {
    with_fixture(|mut db| async move {
        let store = prepare(&mut db).await;
        let principal_id = create_principal(&mut db).await;
        let workspace_id = Uuid::new_v4();
        sqlx::query("INSERT INTO analytics.workspaces (id, name) VALUES ($1, 'Test workspace')")
            .bind(workspace_id)
            .execute(&mut db.owner)
            .await
            .expect("seed a workspace through the trusted connection");
        sqlx::query(
            "INSERT INTO analytics.workspace_members (workspace_id, principal_id, role) VALUES ($1, $2, 'admin')",
        )
        .bind(workspace_id)
        .bind(principal_id)
        .execute(&mut db.owner)
        .await
        .expect("seed an explicit workspace grant");

        link_identity(
            &mut db,
            "https://workos.example.test",
            "workos-subject",
            principal_id,
        )
        .await;
        link_identity(
            &mut db,
            "https://other-oidc.example.test",
            "other-subject",
            principal_id,
        )
        .await;

        for (issuer, subject) in [
            ("https://workos.example.test", "workos-subject"),
            ("https://other-oidc.example.test", "other-subject"),
        ] {
            assert_eq!(
                store
                    .resolve_identity(issuer, subject)
                    .await
                    .expect("resolve an independently linked OIDC identity"),
                Some(principal_id)
            );
        }

        let membership = sqlx::query(
            "SELECT principal_id, role FROM analytics.workspace_members WHERE workspace_id = $1",
        )
        .bind(workspace_id)
        .fetch_one(&mut db.owner)
        .await
        .expect("read the original workspace grant");
        assert_eq!(membership.get::<Uuid, _>("principal_id"), principal_id);
        assert_eq!(membership.get::<String, _>("role"), "admin");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM analytics.workspace_members WHERE workspace_id = $1",
            )
            .bind(workspace_id)
            .fetch_one(&mut db.owner)
            .await
            .expect("count workspace grants after adding the second identity"),
            1
        );
    })
    .await;
}

#[tokio::test]
async fn runtime_cannot_read_or_mutate_identity_links_or_workspace_grants() {
    with_fixture(|mut db| async move {
        let store = prepare(&mut db).await;
        let principal_id = create_principal(&mut db).await;
        let workspace_id = Uuid::new_v4();
        sqlx::query("INSERT INTO analytics.workspaces (id, name) VALUES ($1, 'Test workspace')")
            .bind(workspace_id)
            .execute(&mut db.owner)
            .await
            .expect("seed a workspace through the trusted connection");
        sqlx::query(
            "INSERT INTO analytics.workspace_members (workspace_id, principal_id, role) VALUES ($1, $2, 'reader')",
        )
        .bind(workspace_id)
        .bind(principal_id)
        .execute(&mut db.owner)
        .await
        .expect("seed a workspace grant through the trusted connection");
        link_identity(
            &mut db,
            "https://login.example.test",
            "protected-subject",
            principal_id,
        )
        .await;

        assert_eq!(
            store
                .resolve_identity("https://login.example.test", "protected-subject")
                .await
                .expect("the runtime can use the narrow identity resolver"),
            Some(principal_id)
        );
        let mut runtime = PgConnection::connect_with(&db.runtime)
            .await
            .expect("connect using the restricted application role");

        for statement in [
            "SELECT * FROM analytics.identity_links",
            "INSERT INTO analytics.identity_links (issuer, subject, principal_id) VALUES ('https://attacker.example.test', 'attacker', '00000000-0000-0000-0000-000000000001')",
            "UPDATE analytics.identity_links SET enabled = false",
            "DELETE FROM analytics.identity_links",
            "INSERT INTO analytics.principals (id) VALUES ('00000000-0000-0000-0000-000000000002')",
            "INSERT INTO analytics.workspace_members (workspace_id, principal_id, role) VALUES ('00000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000004', 'admin')",
            "UPDATE analytics.workspace_members SET role = 'admin'",
            "DELETE FROM analytics.workspace_members",
        ] {
            let error = sqlx::query(statement)
                .execute(&mut runtime)
                .await
                .expect_err("runtime SQL must not access or mutate identity and grant tables");
            assert_eq!(
                error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .map(|code| code.into_owned()),
                Some("42501".to_owned()),
                "runtime operation should fail for lack of privilege: {statement}"
            );
        }
        runtime.close().await.expect("close the restricted connection");
    })
    .await;
}

#[tokio::test]
async fn database_errors_have_safe_display_text() {
    let error =
        AuthStoreError::Database(sqlx::Error::Protocol("secret database detail".to_owned()));
    assert_eq!(error.to_string(), "authentication storage is unavailable");
    assert_eq!(format!("{error:?}"), "Database([redacted])");

    let attempt = LoginAttempt {
        pkce_verifier: "secret-verifier".to_owned(),
        nonce: "secret-nonce".to_owned(),
    };
    assert!(!format!("{attempt:?}").contains("secret"));

    let session = BrowserSession {
        principal_id: Uuid::nil(),
        csrf_token: "secret-csrf-token".to_owned(),
    };
    assert!(!format!("{session:?}").contains("secret"));
}
