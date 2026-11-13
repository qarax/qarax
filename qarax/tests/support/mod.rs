//! Shared harness for the HTTP integration tests: a real Axum server and its
//! background tasks, backed by a fresh Postgres database per test.

// Each test file is its own crate and uses a different subset of these helpers.
#![allow(dead_code)]

use std::{
    net::TcpListener as StdTcpListener,
    sync::{
        LazyLock,
        atomic::{AtomicU8, Ordering},
    },
    thread::JoinHandle,
};

use ::common::telemtry::{get_subscriber, init_subscriber};
use qarax::{
    configuration::{
        AuthSettings, DatabaseSettings, default_control_plane_architecture, get_configuration,
    },
    startup::run,
};
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::{runtime::Runtime, sync::oneshot};
use uuid::Uuid;

static TRACING: LazyLock<()> = LazyLock::new(|| {
    let level = "info".to_string();
    let name = "test".to_string();
    if std::env::var("TEST_LOG").is_ok() {
        init_subscriber(get_subscriber(name, level, std::io::stdout));
    } else {
        init_subscriber(get_subscriber(name, level, std::io::sink));
    }
});

pub struct TestApp {
    pub address: String,
    /// Pool for direct database access from the test.
    pub pool: PgPool,
    db_name: String,
    app: Option<AppThread>,
}

struct AppThread {
    shutdown: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

/// Spawns the control plane with authentication disabled, whatever the
/// environment says.
pub async fn spawn_app() -> TestApp {
    spawn_app_with_auth(AuthSettings {
        enabled: false,
        tokens: vec![],
    })
    .await
}

pub async fn spawn_app_with_auth(auth: AuthSettings) -> TestApp {
    LazyLock::force(&TRACING);
    let mut configuration = get_configuration().expect("Failed to read configuration");
    configuration.database.name = Uuid::new_v4().to_string();
    let pool = configure_database(&configuration.database).await;

    let listener = StdTcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());

    // The app gets its own runtime, pool and listener. If it shared the test's
    // runtime, TestApp::drop (which blocks that runtime) could stall one of its
    // pool connections mid-handshake, and DROP DATABASE would then wait for
    // Postgres' 60 s authentication_timeout.
    let (shutdown, shutdown_rx) = oneshot::channel();
    let database = configuration.database.clone();
    let handle = std::thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let app_pool = PgPool::connect(&database.connection_string())
                .await
                .expect("Failed to connect to Postgres");
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let server = run(
                listener,
                app_pool,
                database,
                configuration.vm_defaults,
                configuration.scheduling,
                configuration.sandbox,
                auth,
                configuration.ha,
                default_control_plane_architecture(),
            )
            .await
            .expect("Failed to start the control plane");
            tokio::select! {
                result = server.into_future() => result.expect("Server failed"),
                _ = shutdown_rx => {}
            }
        });
        // Dropping the runtime cancels the background tasks and closes the
        // app pool's connections.
    });

    TestApp {
        address,
        pool,
        db_name: configuration.database.name,
        app: Some(AppThread { shutdown, handle }),
    }
}

async fn configure_database(config: &DatabaseSettings) -> PgPool {
    let mut connection = PgConnection::connect(&config.connection_string_without_db())
        .await
        .expect("Failed to connect to Postgres");
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.name).as_str())
        .await
        .expect("Failed to create database");
    let pool = PgPool::connect(&config.connection_string())
        .await
        .expect("Failed to connect to Postgres");
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .expect("Failed to migrate the database");
    pool
}

impl Drop for TestApp {
    fn drop(&mut self) {
        if let Some(app) = self.app.take() {
            let _ = app.shutdown.send(());
            let _ = app.handle.join();
        }
        let db_name = self.db_name.clone();
        let _ = std::thread::spawn(move || {
            Runtime::new().unwrap().block_on(async {
                let config = get_configuration().expect("Failed to read configuration");
                let mut conn = PgConnection::connect_with(&config.database.without_db())
                    .await
                    .expect("Failed to connect to Postgres");
                conn.execute(format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#).as_str())
                    .await
                    .expect("Failed to drop database");
            })
        })
        .join();
    }
}

static NEXT_HOST: AtomicU8 = AtomicU8::new(1);

/// Registers a host through the API. Each host gets its own loopback address
/// (`hosts.address` is unique) and port 1, where no qarax-node listens, so node
/// calls fail fast.
pub async fn add_host(client: &reqwest::Client, address: &str, name: &str) -> Uuid {
    let host_address = format!("127.0.0.{}", NEXT_HOST.fetch_add(1, Ordering::Relaxed));
    let res = client
        .post(format!("{address}/hosts"))
        .json(&json!({
            "name": name,
            "address": host_address,
            "port": 1,
            "host_user": "root"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    Uuid::parse_str(&res.text().await.unwrap()).unwrap()
}

/// Registers a host and marks it UP so the scheduler can place VMs on it.
pub async fn add_up_host(client: &reqwest::Client, address: &str, name: &str) -> Uuid {
    let host_id = add_host(client, address, name).await;
    let res = client
        .patch(format!("{address}/hosts/{host_id}"))
        .json(&json!({"status": "up"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    host_id
}

/// Creates a VM through the API and returns its id.
pub async fn create_vm(client: &reqwest::Client, address: &str, body: serde_json::Value) -> Uuid {
    let res = client
        .post(format!("{address}/vms"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED, "VM creation failed");
    res.json().await.unwrap()
}

/// Forces a VM's status in the database, for guards that need a state only a
/// real node could reach.
pub async fn set_vm_status(pool: &PgPool, vm_id: Uuid, status: &str) {
    sqlx::query("UPDATE vms SET status = $1::vm_status WHERE id = $2")
        .bind(status)
        .bind(vm_id)
        .execute(pool)
        .await
        .unwrap();
}
