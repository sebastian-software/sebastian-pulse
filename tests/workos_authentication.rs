//! Synthetic WorkOS API contracts plus real HTTP/PostgreSQL authorization.
//! These tests are deliberately not evidence of a live WorkOS tenant login.
use async_trait::async_trait;
use axum::http::StatusCode;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use sebastian_pulse::{
    app::Service,
    auth::{
        AuthService, AuthStore,
        workos::{Access, WorkosConfig, WorkosProvider},
    },
    config::Environment,
    database::create_pool,
    migrations::{load_migrations, migrate},
    workspaces::{NewProject, ProjectKind, WorkspaceError, WorkspaceStore},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::net::TcpListener;
use url::Url;
use uuid::Uuid;
use workos::transport::{HttpRequest, HttpResponse, HttpTransport, TransportError};
#[allow(dead_code)]
mod support;
use support::*;

#[derive(Default)]
struct Fixture {
    active: AtomicBool,
    write: AtomicBool,
    unavailable: AtomicBool,
    membership_unavailable: AtomicBool,
    revoked: AtomicBool,
    verifier: Mutex<String>,
}
fn fixture_user() -> Value {
    json!({
        "object": "user",
        "id": "user_fixture",
        "email": "fixture@example.invalid",
        "email_verified": true,
        "created_at": "2026-10-20T00:00:00Z",
        "updated_at": "2026-10-20T00:00:00Z",
    })
}
fn jwt(claims: Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("initial".into());
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(include_bytes!("fixtures/oidc-insecure-test-key.pem")).unwrap(),
    )
    .unwrap()
}
#[async_trait]
impl HttpTransport for Fixture {
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, TransportError> {
        let url = Url::parse(&req.url).unwrap();
        assert_eq!(url.origin().ascii_serialization(), "https://api.workos.com");
        let mut status = StatusCode::OK;
        let now = jsonwebtoken::get_current_timestamp();
        let user = fixture_user();
        let body = if self.unavailable.load(Ordering::Relaxed)
            || (url.path() == "/user_management/organization_memberships"
                && self.membership_unavailable.load(Ordering::Relaxed))
        {
            status = StatusCode::SERVICE_UNAVAILABLE;
            json!({"message":"fixture unavailable"})
        } else {
            match url.path() {
                "/user_management/authenticate" => {
                    let body: Value = serde_json::from_slice(req.body.as_ref().unwrap()).unwrap();
                    assert_eq!(body["client_id"], "client_fixture");
                    if body["grant_type"] == "authorization_code" {
                        *self.verifier.lock().unwrap() =
                            body["code_verifier"].as_str().unwrap().to_owned();
                    } else {
                        assert_eq!(body["grant_type"], "refresh_token");
                        assert_eq!(body["refresh_token"], "refresh_fixture");
                    }
                    json!({"user":user,"access_token":jwt(json!({"iss":"https://api.workos.com/","sub":"user_fixture","sid":"session_fixture","org_id":"org_fixture","iat":now,"exp":now+300})),"refresh_token":"refresh_fixture"})
                }
                "/user_management/organization_memberships" => {
                    assert!(
                        url.query_pairs()
                            .any(|(k, v)| k == "user_id" && v == "user_fixture")
                    );
                    assert!(
                        url.query_pairs()
                            .any(|(k, v)| k == "organization_id" && v == "org_fixture")
                    );
                    let data = if self.active.load(Ordering::Relaxed) {
                        vec![
                            json!({"object":"organization_membership","id":"om_fixture","user_id":"user_fixture","organization_id":"org_fixture","status":"active","directory_managed":false,"role":{"slug":"member"},"roles":[],"user":user,"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z"}),
                        ]
                    } else {
                        vec![]
                    };
                    json!({"object":"list","data":data,"list_metadata":{"before":null,"after":null}})
                }
                "/authorization/organizations/org_fixture/roles/member" => {
                    let permissions = if self.write.load(Ordering::Relaxed) {
                        vec!["pulse:read", "pulse:write"]
                    } else {
                        vec!["pulse:read"]
                    };
                    json!({"object":"role","id":"role_fixture","slug":"member","name":"Fixture","type":"EnvironmentRole","resource_type_slug":"organization","permissions":permissions,"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z"})
                }
                "/user_management/sessions/revoke" => {
                    self.revoked.store(true, Ordering::Relaxed);
                    status = StatusCode::NO_CONTENT;
                    json!(null)
                }
                _ => panic!("unexpected WorkOS fixture path"),
            }
        };
        Ok(HttpResponse {
            status,
            headers: Default::default(),
            body: serde_json::to_vec(&body).unwrap().into(),
        })
    }
}

async fn setup(db: &mut DbFixture) -> (sqlx::PgPool, Uuid, Uuid) {
    migrate(db.migration.clone(), &load_migrations().unwrap())
        .await
        .unwrap();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    for workspace in [first, second] {
        sqlx::query("INSERT INTO analytics.workspaces(id,name) VALUES($1,'Fixture')")
            .bind(workspace)
            .execute(&mut db.owner)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO analytics.workos_workspaces(client_id,organization_id,workspace_id) VALUES('client_fixture','org_fixture',$1)").bind(first).execute(&mut db.owner).await.unwrap();
    (create_pool(db.runtime.clone()), first, second)
}
fn access(write: bool) -> Access {
    Access {
        user_id: "user_fixture".into(),
        organization_id: "org_fixture".into(),
        can_write: write,
    }
}

#[tokio::test]
async fn managed_access_uses_stable_bindings_and_transaction_local_rls_without_local_grants() {
    with_fixture(check_managed_access).await;
}

async fn check_managed_access(mut db: DbFixture) {
    let (pool, first, second) = setup(&mut db).await;
    let auth = AuthStore::new(pool.clone());
    let mut unknown = access(true);
    unknown.organization_id = "org_unknown".into();
    assert!(
        auth.workos_caller("client_fixture", unknown)
            .await
            .unwrap()
            .is_none()
    );
    let (one, two) = tokio::join!(
        auth.workos_caller("client_fixture", access(true)),
        auth.workos_caller("client_fixture", access(true))
    );
    let one = one.unwrap().unwrap();
    let two = two.unwrap().unwrap();
    assert_eq!(one.principal_id, two.principal_id);
    let store = WorkspaceStore::new(pool.clone());
    assert!(
        matches!(
            store.workspace(one.principal_id, first).await,
            Err(WorkspaceError::Denied)
        ),
        "the identity binding creates no local grant"
    );
    let managed = store.for_caller(one);
    assert!(managed.workspace(one.principal_id, first).await.is_ok());
    managed
        .create_project(
            one.principal_id,
            first,
            NewProject {
                name: "Managed".into(),
                kind: ProjectKind::Website,
            },
        )
        .await
        .unwrap();
    sqlx::query("INSERT INTO analytics.workspace_members(workspace_id,principal_id,role) VALUES($1,$2,'admin'),($3,$2,'admin')").bind(first).bind(one.principal_id).bind(second).execute(&mut db.owner).await.unwrap();
    assert!(
        matches!(
            managed.workspace(one.principal_id, second).await,
            Err(WorkspaceError::Denied)
        ),
        "local membership cannot broaden managed workspace scope"
    );
    let read = auth
        .workos_caller("client_fixture", access(false))
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            store
                .for_caller(read)
                .create_project(
                    read.principal_id,
                    first,
                    NewProject {
                        name: "Denied".into(),
                        kind: ProjectKind::Website
                    }
                )
                .await,
            Err(WorkspaceError::Denied)
        ),
        "local admin cannot restore removed WorkOS write permission"
    );
    let other = Uuid::new_v4();
    assert!(matches!(
        managed.workspace(other, first).await,
        Err(WorkspaceError::Denied)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "managed tenant context does not leak across pooled transactions"
    );
    assert!(
        sqlx::query("SELECT * FROM analytics.workos_workspaces")
            .fetch_all(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "INSERT INTO analytics.workos_workspaces VALUES('client_fixture','org_other',$1)"
        )
        .bind(second)
        .execute(&pool)
        .await
        .is_err()
    );
    pool.close().await;
}

async fn tool(client: &reqwest::Client, base: &str, token: &str, name: &str, args: Value) -> Value {
    client.post(format!("{base}/mcp")).bearer_auth(token).header("accept","application/json, text/event-stream").header("mcp-protocol-version","2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}})).send().await.unwrap().json().await.unwrap()
}

#[tokio::test]
async fn native_browser_and_mcp_enforce_csrf_tenant_scope_permission_changes_and_outages() {
    with_fixture(check_native_authentication).await;
}

async fn check_native_authentication(mut db: DbFixture) {
    let (pool, first, second) = setup(&mut db).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let env = Environment::from([
        ("APP_ENV".into(), "test".into()),
        ("AUTH_PUBLIC_URL".into(), base.clone()),
        ("WORKOS_API_KEY".into(), "sk_fixture".into()),
        ("WORKOS_CLIENT_ID".into(), "client_fixture".into()),
        (
            "WORKOS_AUTHKIT_URL".into(),
            "https://fixture.authkit.app".into(),
        ),
        ("WORKOS_COOKIE_PASSWORD".into(), "ab".repeat(32)),
    ]);
    let fixture = Arc::new(Fixture {
        active: AtomicBool::new(true),
        write: AtomicBool::new(true),
        ..Default::default()
    });
    let provider = WorkosProvider::for_protocol_fixture(
        WorkosConfig::from_env(&env).unwrap().unwrap(),
        fixture.clone(),
        serde_json::from_str(include_str!("fixtures/oidc-jwks.json")).unwrap(),
    )
    .unwrap();
    let auth = AuthService::with_workos_provider(pool.clone(), provider).unwrap();
    let router = Service::new(
        pool.clone(),
        load_migrations().unwrap(),
        "workos-fixture".into(),
    )
    .router_with_auth(Some(auth));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let login = client
        .get(format!("{base}/auth/login"))
        .send()
        .await
        .unwrap();
    assert!(login.status().is_redirection());
    let location = Url::parse(login.headers()["location"].to_str().unwrap()).unwrap();
    let state = location
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let binding = login.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert_eq!(
        client
            .get(format!("{base}/auth/callback?code=fixture&state={state}"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let response = client
        .get(format!("{base}/auth/callback?code=fixture&state={state}"))
        .header("cookie", binding)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_redirection());
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    assert_eq!(fixture.verifier.lock().unwrap().len(), 43);
    assert_eq!(
        client
            .get(format!("{base}/auth/callback?code=fixture&state={state}"))
            .header("cookie", binding)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let session: Value = client
        .get(format!("{base}/auth/session"))
        .header("cookie", &cookies)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(session["workspace_id"], first.to_string());
    assert_eq!(session["can_write"], true);
    let csrf = session["csrf_token"].as_str().unwrap();
    // A successful token rotation must reach the browser even if the following
    // membership lookup fails, or the next request would reuse an old refresh token.
    let now = jsonwebtoken::get_current_timestamp();
    let expired = workos::seal_session(
        &workos::SessionData {
            access_token: jwt(json!({
                "iss": "https://api.workos.com/",
                "sub": "user_fixture",
                "sid": "session_fixture",
                "org_id": "org_fixture",
                "iat": now - 300,
                "exp": now - 1,
            }))
            .into(),
            refresh_token: "refresh_fixture".into(),
            user: Some(serde_json::from_value(fixture_user()).unwrap()),
            impersonator: None,
        },
        &"ab".repeat(32),
    )
    .unwrap();
    fixture
        .membership_unavailable
        .store(true, Ordering::Relaxed);
    let unavailable = client
        .get(format!("{base}/auth/session"))
        .header(
            "cookie",
            format!("pulse_session={expired}; pulse_csrf={csrf}"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(unavailable.status(), 503);
    assert!(
        unavailable.headers().get_all("set-cookie").iter().any(|v| {
            let value = v.to_str().unwrap();
            value.starts_with("pulse_session=") && !value.contains("Max-Age=0")
        }),
        "the renewed session survives a transient authorization outage"
    );
    fixture
        .membership_unavailable
        .store(false, Ordering::Relaxed);
    assert_eq!(
        client
            .get(format!("{base}/v1/workspaces/{second}"))
            .header("cookie", &cookies)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let project = json!({"name":"Browser","kind":"website"});
    let endpoint = format!("{base}/v1/workspaces/{first}/projects");
    assert_eq!(
        client
            .post(&endpoint)
            .header("cookie", &cookies)
            .json(&project)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .post(&endpoint)
            .header("cookie", &cookies)
            .header("origin", &base)
            .header("x-csrf-token", csrf)
            .json(&project)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let now = jsonwebtoken::get_current_timestamp();
    let read = jwt(
        json!({"iss":"https://fixture.authkit.app","aud":format!("{base}/mcp"),"sub":"user_fixture","org_id":"org_fixture","scope":"pulse:read","iat":now,"exp":now+300}),
    );
    let write = jwt(
        json!({"iss":"https://fixture.authkit.app","aud":format!("{base}/mcp"),"sub":"user_fixture","org_id":"org_fixture","scope":"pulse:read pulse:write","iat":now,"exp":now+300}),
    );
    assert!(
        tool(
            &client,
            &base,
            &read,
            "workspace_get",
            json!({"workspace_id":first})
        )
        .await
        .get("result")
        .is_some()
    );
    assert!(
        tool(
            &client,
            &base,
            &read,
            "workspace_get",
            json!({"workspace_id":second})
        )
        .await
        .get("error")
        .is_some()
    );
    assert!(
        tool(
            &client,
            &base,
            &read,
            "projects_create",
            json!({"workspace_id":first,"name":"MCP","kind":"cli"})
        )
        .await
        .get("error")
        .is_some()
    );
    fixture.write.store(false, Ordering::Relaxed);
    assert!(
        tool(
            &client,
            &base,
            &write,
            "projects_create",
            json!({"workspace_id":first,"name":"MCP","kind":"cli"})
        )
        .await
        .get("error")
        .is_some()
    );
    assert_eq!(
        client
            .post(&endpoint)
            .header("cookie", &cookies)
            .header("origin", &base)
            .header("x-csrf-token", csrf)
            .json(&project)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    fixture.active.store(false, Ordering::Relaxed);
    assert_eq!(
        client
            .get(format!("{base}/v1/workspaces/{first}"))
            .header("cookie", &cookies)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    fixture.unavailable.store(true, Ordering::Relaxed);
    assert_eq!(
        client
            .get(format!("{base}/v1/workspaces/{first}"))
            .header("cookie", &cookies)
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    fixture.unavailable.store(false, Ordering::Relaxed);
    let logout = client
        .post(format!("{base}/auth/logout"))
        .header("cookie", &cookies)
        .header("origin", &base)
        .header("x-csrf-token", csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 204);
    assert!(fixture.revoked.load(Ordering::Relaxed));
    assert!(
        logout
            .headers()
            .get_all("set-cookie")
            .iter()
            .all(|v| v.to_str().unwrap().contains("Max-Age=0"))
    );
    server.abort();
    pool.close().await;
}
