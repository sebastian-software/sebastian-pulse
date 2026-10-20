use analytics_service::{app::Service, config, database, log, migrations};
use serde_json::json;
use std::{collections::HashMap, future::IntoFuture, process::ExitCode, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, time::timeout};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

async fn run() -> Result<(), ()> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => serve(&env).await,
        [command] if command == "serve" => serve(&env).await,
        [command] if command == "migrate" => {
            let options = config::database_config(&env, config::Purpose::Migration)
                .map_err(|_| log::event("migration_failed", json!({"code":"CONFIGURATION"})))?;
            let migrations = migrations::load_migrations()
                .map_err(|_| log::event("migration_failed", json!({"code":"MIGRATION_FILES"})))?;
            let applied = migrations::migrate(options, &migrations)
                .await
                .map_err(|_| log::event("migration_failed", json!({"code":"MIGRATION"})))?;
            log::event("migrations_complete", json!({"applied":applied}));
            Ok(())
        }
        [command] if command == "healthcheck" => probe(&env, false).await,
        [command] if command == "smoke" => probe(&env, true).await,
        _ => {
            log::event("invalid_command", json!({}));
            Err(())
        }
    }
}

async fn probe(env: &HashMap<String, String>, smoke: bool) -> Result<(), ()> {
    let base = if smoke {
        env.get("SMOKE_BASE_URL")
            .cloned()
            .unwrap_or_else(|| "http://127.0.0.1:3000".into())
    } else {
        let port = env.get("PORT").map(String::as_str).unwrap_or("3000");
        let port: u16 = port.parse().ok().filter(|port| *port > 0).ok_or(())?;
        format!("http://127.0.0.1:{port}")
    };
    let base = url::Url::parse(&base).map_err(|_| ())?;
    if !matches!(base.scheme(), "http" | "https")
        || !base.username().is_empty()
        || base.password().is_some()
    {
        return Err(());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ())?;
    let paths: &[&str] = if smoke {
        &["/health/live", "/health/ready"]
    } else {
        &["/health/ready"]
    };
    for path in paths {
        let response = client
            .get(base.join(path).map_err(|_| ())?)
            .send()
            .await
            .map_err(|_| ())?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(());
        }
        if smoke {
            let value: serde_json::Value = response.json().await.map_err(|_| ())?;
            log::event("smoke_passed", json!({"path":path, "result":value}));
        }
    }
    Ok(())
}

async fn serve(env: &HashMap<String, String>) -> Result<(), ()> {
    let config = config::runtime_config(env)
        .map_err(|error| log::event("startup_failed", json!({"reason":error.to_string()})))?;
    let migrations = migrations::load_migrations()
        .map_err(|_| log::event("startup_failed", json!({"code":"MIGRATION_FILES"})))?;
    let pool = database::create_pool(config.database);
    let service = Service::new(pool.clone(), migrations, config.release.clone());
    let listener = TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, config.port))
        .await
        .map_err(|_| log::event("server_failed", json!({"code":"BIND"})))?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = axum::serve(listener, service.router())
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .into_future();
    tokio::pin!(server);
    log::event(
        "service_started",
        json!({"port":config.port,"release":config.release}),
    );
    tokio::select! {
        result = &mut server => {
            let _ = result;
            log::event("server_failed", json!({"code":"SERVER"}));
            let _ = timeout(Duration::from_secs(20), pool.close()).await;
            Err(())
        }
        result = shutdown_signal() => {
            result.map_err(|_| log::event("shutdown_failed", json!({"code":"SIGNAL"})))?;
            service.drain();
            log::event("shutdown_started", json!({}));
            let _ = shutdown_tx.send(());
            match timeout(Duration::from_secs(20), async {
                server.await.map_err(|_| ())?;
                pool.close().await;
                Ok::<(), ()>(())
            }).await {
                Ok(Ok(())) => { log::event("shutdown_complete", json!({})); Ok(()) }
                Ok(Err(())) => { log::event("shutdown_failed", json!({"code":"SERVER"})); Err(()) }
                Err(_) => { log::event("shutdown_deadline", json!({})); std::process::exit(1); }
            }
        }
    }
}

async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}
