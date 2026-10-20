//! Local protocol fixture tests; these do not substitute for an actual provider/client smoke.
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::HeaderMap,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use sebastian_analytics::{
    app::Service,
    auth::{AuthConfig, AuthService},
    config::Environment,
    database::create_pool,
    migrations::{load_migrations, migrate},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};
use tokio::net::TcpListener;
use uuid::Uuid;
#[allow(dead_code)]
mod support;
use support::*;

const PRIVATE: &[u8] = include_bytes!("fixtures/oidc-insecure-test-key.pem");
const ROTATED: &[u8] = include_bytes!("fixtures/oidc-insecure-test-key-rotated.pem");
struct Provider {
    issuer: String,
    login: Mutex<Option<(String, String, String)>>,
    verified: AtomicBool,
    bad_nonce: AtomicBool,
    bad_subject: AtomicBool,
    rotated: AtomicBool,
    unavailable: AtomicBool,
    discoveries: AtomicU32,
}
impl Provider {
    fn jwt(&self, mut claims: Value) -> String {
        if claims.get("iss").is_none() {
            claims["iss"] = json!(self.issuer);
        }
        let rotated = self.rotated.load(Ordering::Relaxed);
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(if rotated { "rotated" } else { "initial" }.into());
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(if rotated { ROTATED } else { PRIVATE }).unwrap(),
        )
        .unwrap()
    }
    fn bearer(&self, resource: &str, subject: &str, scope: &str) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        // A standard audience array exercises another provider's representation (e.g. Rauthy).
        self.jwt(json!({"aud":["mcp-fixture-client",resource],"sub":subject,"scope":scope,"iat":now,"exp":now+300}))
    }
}
async fn start_provider() -> (Arc<Provider>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let p = Arc::new(Provider {
        issuer: format!("http://{}/auth/v1", listener.local_addr().unwrap()),
        login: Mutex::new(None),
        verified: AtomicBool::new(true),
        bad_nonce: AtomicBool::new(false),
        bad_subject: AtomicBool::new(false),
        rotated: AtomicBool::new(false),
        unavailable: AtomicBool::new(false),
        discoveries: AtomicU32::new(0),
    });
    let router = Router::new()
        .route("/auth/v1/.well-known/openid-configuration", get(discovery))
        .route("/auth/v1/jwks", get(jwks))
        .route("/auth/v1/token", post(token))
        .route("/auth/v1/userinfo", get(userinfo))
        .with_state(p.clone());
    (
        p,
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
    )
}
async fn discovery(State(p): State<Arc<Provider>>) -> axum::response::Response {
    use axum::response::IntoResponse;
    p.discoveries.fetch_add(1, Ordering::Relaxed);
    if p.unavailable.load(Ordering::Relaxed) {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(
        json!({"issuer":p.issuer,"authorization_endpoint":format!("{}/authorize",p.issuer),"token_endpoint":format!("{}/token",p.issuer),"userinfo_endpoint":format!("{}/userinfo",p.issuer),"jwks_uri":format!("{}/jwks",p.issuer),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"]}),
    ).into_response()
}
async fn jwks(State(p): State<Arc<Provider>>) -> Json<Value> {
    let keys: Value = serde_json::from_str(include_str!("fixtures/oidc-jwks.json")).unwrap();
    Json(json!({"keys":[keys["keys"][usize::from(p.rotated.load(Ordering::Relaxed))]]}))
}
async fn token(State(p): State<Arc<Provider>>, headers: HeaderMap, body: Bytes) -> Json<Value> {
    let form: HashMap<String, String> = url::form_urlencoded::parse(&body).into_owned().collect();
    assert!(
        headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("Basic ")
    );
    assert_eq!(form.get("grant_type").unwrap(), "authorization_code");
    let (nonce, challenge, callback) = p
        .login
        .lock()
        .unwrap()
        .take()
        .expect("one-use fixture code");
    assert_eq!(form.get("code").unwrap(), "fixture-code");
    assert_eq!(form.get("redirect_uri").unwrap(), &callback);
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(
            form.get("code_verifier").unwrap().as_bytes()
        )),
        challenge
    );
    let now = jsonwebtoken::get_current_timestamp();
    let id=p.jwt(json!({"aud":"browser-fixture","sub":"admin","iat":now,"exp":now+300,"nonce":if p.bad_nonce.load(Ordering::Relaxed){"wrong"}else{&nonce}}));
    Json(
        json!({"access_token":"fixture-userinfo-token","token_type":"Bearer","expires_in":300,"id_token":id}),
    )
}
async fn userinfo(State(p): State<Arc<Provider>>, headers: HeaderMap) -> Json<Value> {
    assert_eq!(
        headers.get("authorization").unwrap(),
        "Bearer fixture-userinfo-token"
    );
    Json(
        json!({"sub":if p.bad_subject.load(Ordering::Relaxed){"other-user"}else{"admin"},"email":"admin@example.invalid","email_verified":p.verified.load(Ordering::Relaxed)}),
    )
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}
async fn start_service(pool: PgPool, p: &Provider) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let env = Environment::from([
        ("APP_ENV".into(), "test".into()),
        ("AUTH_ISSUER_URL".into(), p.issuer.clone()),
        ("AUTH_PUBLIC_URL".into(), base.clone()),
        ("AUTH_CLIENT_ID".into(), "browser-fixture".into()),
        (
            "AUTH_CLIENT_SECRET".into(),
            "insecure-fixture-secret".into(),
        ),
    ]);
    let auth =
        AuthService::new(pool.clone(), AuthConfig::from_env(&env).unwrap().unwrap()).unwrap();
    let service = Service::new(pool, load_migrations().unwrap(), "auth-test".into());
    let router = service.router_with_auth(Some(auth));
    (
        base,
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
    )
}
async fn seed(db: &mut DbFixture, issuer: &str) -> (PgPool, Uuid, Uuid, Uuid) {
    migrate(db.migration.clone(), &load_migrations().unwrap())
        .await
        .unwrap();
    let admin = Uuid::new_v4();
    let reader = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    for (id, subject, role) in [(admin, "admin", "admin"), (reader, "reader", "reader")] {
        sqlx::query("INSERT INTO analytics.principals(id) VALUES($1)")
            .bind(id)
            .execute(&mut db.owner)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO analytics.identity_links(issuer,subject,principal_id) VALUES($1,$2,$3)",
        )
        .bind(issuer)
        .bind(subject)
        .bind(id)
        .execute(&mut db.owner)
        .await
        .unwrap();
        if role == "admin" {
            sqlx::query("INSERT INTO analytics.workspaces(id,name) VALUES($1,'Fixture')")
                .bind(workspace)
                .execute(&mut db.owner)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO analytics.workspace_members(workspace_id,principal_id,role) VALUES($1,$2,$3)").bind(workspace).bind(id).bind(role).execute(&mut db.owner).await.unwrap();
    }
    (create_pool(db.runtime.clone()), admin, reader, workspace)
}
async fn attempt(c: &reqwest::Client, base: &str, p: &Provider) -> (String, String) {
    let response = c.get(format!("{base}/auth/login")).send().await.unwrap();
    assert_eq!(response.status(), 303);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let url = url::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
    let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(q["code_challenge_method"], "S256");
    assert!(q["scope"].split_whitespace().any(|s| s == "email"));
    assert!(!q["scope"].contains("offline_access"));
    *p.login.lock().unwrap() = Some((
        q["nonce"].clone(),
        q["code_challenge"].clone(),
        q["redirect_uri"].clone(),
    ));
    let mut callback = url::Url::parse(&format!("{base}/auth/callback")).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &q["state"])
        .append_pair("code", "fixture-code");
    (cookie, callback.to_string())
}
#[tokio::test]
async fn browser_login_binds_code_checks_identity_and_enforces_session_csrf_and_grants() {
    with_fixture(|mut db| async move {
        let (provider, pserver) = start_provider().await;
        let (pool, admin, _, wid) = seed(&mut db, &provider.issuer).await;
        let (base, server) = start_service(pool.clone(), &provider).await;
        let c = client();
        assert_eq!(
            c.get(format!("{base}/not-found"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        assert_eq!(
            c.get(format!("{base}/auth/session"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        for rejection in ["nonce", "email", "subject"] {
            provider
                .bad_nonce
                .store(rejection == "nonce", Ordering::Relaxed);
            provider
                .verified
                .store(rejection != "email", Ordering::Relaxed);
            provider
                .bad_subject
                .store(rejection == "subject", Ordering::Relaxed);
            let (cookie, callback) = attempt(&c, &base, &provider).await;
            assert_eq!(
                c.get(callback)
                    .header("cookie", cookie)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                401,
                "{rejection}"
            );
        }
        provider.bad_nonce.store(false, Ordering::Relaxed);
        provider.verified.store(true, Ordering::Relaxed);
        provider.bad_subject.store(false, Ordering::Relaxed);
        let (binding, callback) = attempt(&c, &base, &provider).await;
        assert_eq!(c.get(&callback).send().await.unwrap().status(), 400);
        let response = c
            .get(&callback)
            .header("cookie", &binding)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 303);
        let session = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| v.starts_with("analytics_session="))
            .unwrap();
        assert!(session.contains("HttpOnly") && session.contains("SameSite=Lax"));
        let cookie = session.split(';').next().unwrap().to_owned();
        assert_eq!(
            c.get(&callback)
                .header("cookie", binding)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        let info: Value = c
            .get(format!("{base}/auth/session"))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(info["principal_id"], admin.to_string());
        let url = format!("{base}/v1/workspaces/{wid}/projects");
        let value = json!({"name":"Website","kind":"website"});
        assert_eq!(
            c.post(&url)
                .header("cookie", &cookie)
                .json(&value)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            c.post(&url)
                .header("cookie", &cookie)
                .header("origin", "https://evil.invalid")
                .header("x-csrf-token", info["csrf_token"].as_str().unwrap())
                .json(&value)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        let response = c
            .post(&url)
            .header("cookie", &cookie)
            .header("origin", &base)
            .header("x-csrf-token", info["csrf_token"].as_str().unwrap())
            .json(&value)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            c.get(format!("{base}/v1/workspaces/{}/projects", Uuid::new_v4()))
                .header("cookie", &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            c.post(format!("{base}/auth/logout"))
                .header("cookie", &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            c.post(format!("{base}/auth/logout"))
                .header("cookie", &cookie)
                .header("origin", &base)
                .header("x-csrf-token", info["csrf_token"].as_str().unwrap())
                .send()
                .await
                .unwrap()
                .status(),
            204
        );
        assert_eq!(
            c.get(format!("{base}/auth/session"))
                .header("cookie", &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        server.abort();
        pserver.abort();
        pool.close().await;
    })
    .await;
}
async fn tool(c: &reqwest::Client, base: &str, token: &str, name: &str, args: Value) -> Value {
    let response=c.post(format!("{base}/mcp")).bearer_auth(token).header("accept","application/json, text/event-stream").header("mcp-protocol-version","2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}})).send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, 200, "{text}");
    serde_json::from_str(&text).expect("JSON MCP response")
}
#[tokio::test]
async fn mcp_checks_tokens_each_request_scopes_local_roles_and_revocation() {
    with_fixture(|mut db|async move {
        let (provider,pserver)=start_provider().await;
        let (pool,_,_,wid)=seed(&mut db,&provider.issuer).await;
        let (base,server)=start_service(pool.clone(),&provider).await;let c=client();let resource=format!("{base}/mcp");
        let response=c.post(&resource).send().await.unwrap();assert_eq!(response.status(),401);assert!(response.headers()["www-authenticate"].to_str().unwrap().contains("resource_metadata="));
        let metadata:Value=c.get(format!("{base}/.well-known/oauth-protected-resource/mcp")).send().await.unwrap().json().await.unwrap();assert_eq!(metadata["resource"],resource);assert_eq!(metadata["authorization_servers"][0],provider.issuer);
        let now=jsonwebtoken::get_current_timestamp();
        for claims in [json!({"iss":"https://wrong.invalid","aud":resource,"sub":"admin","scope":"mcp:read","iat":now,"exp":now+30}),
            json!({"aud":"another-service","sub":"admin","scope":"mcp:read","iat":now,"exp":now+30}),
            json!({"aud":resource,"sub":"admin","scope":"mcp:read","iat":now-60,"exp":now-1}),
            json!({"aud":resource,"sub":"admin","scope":"mcp:write","iat":now,"exp":now+30})] {
            assert_eq!(c.post(&resource).bearer_auth(provider.jwt(claims)).send().await.unwrap().status(),401);
        }
        assert_eq!(c.post(&resource).bearer_auth(provider.bearer(&resource,"unknown","mcp:read")).send().await.unwrap().status(),403);
        let mut forged_header=Header::new(Algorithm::RS256);forged_header.kid=Some("initial".into());
        let forged=encode(&forged_header,&json!({"iss":provider.issuer,"aud":resource,"sub":"admin","scope":"mcp:read mcp:write","iat":now,"exp":now+300}),&EncodingKey::from_rsa_pem(ROTATED).unwrap()).unwrap();
        assert_eq!(c.post(&resource).bearer_auth(forged).send().await.unwrap().status(),401,"a trusted KID and correct claims cannot substitute for a valid signature");
        let read=provider.bearer(&resource,"admin","mcp:read");let write=provider.bearer(&resource,"admin","mcp:read mcp:write");
        assert_eq!(c.post(&resource).bearer_auth(&write).header("origin","https://evil.invalid").send().await.unwrap().status(),403);
        let list=tool(&c,&base,&read,"projects_list",json!({"workspace_id":wid})).await;assert!(list.get("result").is_some(),"{list}");
        let args=json!({"workspace_id":wid,"name":"CLI","kind":"cli"});
        assert!(tool(&c,&base,&read,"projects_create",args.clone()).await.get("error").is_some());
        let reader=provider.bearer(&resource,"reader","mcp:read mcp:write");
        assert!(tool(&c,&base,&reader,"projects_create",args.clone()).await.get("error").is_some());
        let created=tool(&c,&base,&write,"projects_create",args).await;assert!(created.get("result").is_some(),"{created}");
        assert!(tool(&c,&base,&write,"projects_list",json!({"workspace_id":Uuid::new_v4()})).await.get("error").is_some());
        // A caller cannot smuggle an actor through either transport.
        let injected=tool(&c,&base,&write,"projects_list",json!({"workspace_id":wid,"actor":Uuid::new_v4()})).await;
        assert!(injected.get("error").is_some() || injected["result"]["isError"]==true,"{injected}");
        sqlx::query("DELETE FROM analytics.workspace_members WHERE workspace_id=$1 AND role='admin'").bind(wid).execute(&mut db.owner).await.unwrap();
        assert!(tool(&c,&base,&write,"projects_list",json!({"workspace_id":wid})).await.get("error").is_some());
        sqlx::query("UPDATE analytics.identity_links SET enabled=false WHERE subject='admin'").execute(&mut db.owner).await.unwrap();
        assert_eq!(c.post(&resource).bearer_auth(write).send().await.unwrap().status(),403);
        server.abort();pserver.abort();pool.close().await;
    }).await;
}

#[tokio::test]
async fn cached_keys_rotate_and_failed_refreshes_are_rate_limited() {
    use sebastian_analytics::auth::oidc::OidcProvider;
    let (p, server) = start_provider().await;
    let env = Environment::from([
        ("APP_ENV".into(), "test".into()),
        ("AUTH_ISSUER_URL".into(), p.issuer.clone()),
        (
            "AUTH_PUBLIC_URL".into(),
            "https://analytics.example.invalid".into(),
        ),
        ("AUTH_CLIENT_ID".into(), "browser-fixture".into()),
        ("AUTH_CLIENT_SECRET".into(), "fixture-secret".into()),
    ]);
    let provider = OidcProvider::new(AuthConfig::from_env(&env).unwrap().unwrap()).unwrap();
    let resource = "https://analytics.example.invalid/mcp";
    let original = p.bearer(resource, "admin", "mcp:read");
    assert!(provider.verify_bearer(&original).await.is_ok());
    p.rotated.store(true, Ordering::Relaxed);
    let rotated = p.bearer(resource, "admin", "mcp:read");
    assert!(provider.verify_bearer(&rotated).await.is_err());
    assert_eq!(p.discoveries.load(Ordering::Relaxed), 1);
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    assert!(provider.verify_bearer(&rotated).await.is_ok());
    assert!(provider.verify_bearer(&original).await.is_err());
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    p.unavailable.store(true, Ordering::Relaxed);
    let before = p.discoveries.load(Ordering::Relaxed);
    assert!(provider.verify_bearer(&original).await.is_err());
    assert!(provider.verify_bearer(&original).await.is_err());
    assert_eq!(p.discoveries.load(Ordering::Relaxed), before + 1);
    assert!(provider.verify_bearer(&rotated).await.is_ok());
    server.abort();
}

#[tokio::test]
async fn replacing_the_issuer_preserves_local_principal_and_workspace_grants() {
    with_fixture(|mut db|async move {
        let (old,oldserver)=start_provider().await;
        let (new,newserver)=start_provider().await;
        let (pool,admin,_,wid)=seed(&mut db,&old.issuer).await;
        sqlx::query("INSERT INTO analytics.identity_links(issuer,subject,principal_id) VALUES($1,'new-provider-user',$2)")
            .bind(&new.issuer).bind(admin).execute(&mut db.owner).await.unwrap();
        let (oldbase,oldapp)=start_service(pool.clone(),&old).await;
        let (newbase,newapp)=start_service(pool.clone(),&new).await;
        let c=client();
        let oldtoken=old.bearer(&format!("{oldbase}/mcp"),"admin","mcp:read");
        let newtoken=new.bearer(&format!("{newbase}/mcp"),"new-provider-user","mcp:read");
        let previous=tool(&c,&oldbase,&oldtoken,"workspace_get",json!({"workspace_id":wid})).await;
        let replacement=tool(&c,&newbase,&newtoken,"workspace_get",json!({"workspace_id":wid})).await;
        assert_eq!(previous["result"],replacement["result"]);
        assert!(previous.get("result").is_some());
        // Changing the configured provider does not trust tokens signed by the previous issuer.
        assert_eq!(c.post(format!("{newbase}/mcp")).bearer_auth(&oldtoken).send().await.unwrap().status(),401);
        sqlx::query("UPDATE analytics.identity_links SET enabled=false WHERE issuer=$1").bind(&old.issuer).execute(&mut db.owner).await.unwrap();
        assert_eq!(c.post(format!("{oldbase}/mcp")).bearer_auth(&oldtoken).send().await.unwrap().status(),403);
        assert!(tool(&c,&newbase,&newtoken,"workspace_get",json!({"workspace_id":wid})).await.get("result").is_some());
        oldapp.abort();newapp.abort();oldserver.abort();newserver.abort();pool.close().await;
    }).await;
}
