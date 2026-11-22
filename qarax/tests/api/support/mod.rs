//! Shared harness for the HTTP integration tests: a real Axum server and its
//! background tasks, backed by a fresh Postgres database per test.

use std::{
    net::TcpListener as StdTcpListener,
    sync::{
        LazyLock,
        atomic::{AtomicU8, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use ::common::telemtry::{get_subscriber, init_subscriber};
use qarax::{
    configuration::{
        AuthSettings, DatabaseSettings, default_control_plane_architecture, get_configuration,
    },
    grpc_client::NodeTls,
    startup::run,
};
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool, postgres::PgPoolOptions};
use tokio::{runtime::Runtime, sync::oneshot};
use uuid::Uuid;

pub mod fake_node;

static TRACING: LazyLock<()> = LazyLock::new(|| {
    let level = "info".to_string();
    let name = "test".to_string();
    if std::env::var("TEST_LOG").is_ok() {
        init_subscriber(get_subscriber(name, level, std::io::stdout));
    } else {
        init_subscriber(get_subscriber(name, level, std::io::sink));
    }
});

/// The node TLS files, made by the script operators use: the control plane's,
/// and those of a node registered as 127.0.0.1 (see [`node_server_tls`]).
static NODE_TLS_DIR: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("sh")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../scripts/gen-node-tls.sh"
        ))
        .arg(dir.path())
        .arg("127.0.0.1")
        .status()
        .expect("failed to run gen-node-tls.sh");
    assert!(status.success(), "gen-node-tls.sh failed");
    dir
});

/// The web UI's entry page in [`UI_DIR`].
pub const UI_INDEX: &str = "<!doctype html><title>qarax</title>";

/// A stand-in for the web UI's build, served at /ui: its entry page and one
/// of its hashed files.
static UI_DIR: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), UI_INDEX).unwrap();
    std::fs::create_dir_all(dir.path().join("_app/immutable")).unwrap();
    std::fs::write(dir.path().join("_app/immutable/app.js"), "").unwrap();
    dir
});

/// The TLS a fake node registered as 127.0.0.1 serves with, as qarax-node
/// does: it accepts only the control plane's client certificate.
pub fn node_server_tls() -> tonic::transport::ServerTlsConfig {
    use tonic::transport::{Certificate, Identity, ServerTlsConfig};
    let read =
        |name: &str| std::fs::read(NODE_TLS_DIR.path().join("nodes/127.0.0.1").join(name)).unwrap();
    ServerTlsConfig::new()
        .identity(Identity::from_pem(
            read("server-cert.pem"),
            read("server-key.pem"),
        ))
        .client_ca_root(Certificate::from_pem(read("ca-cert.pem")))
}

pub struct TestApp {
    /// The API's base URL (`/v1` on the server).
    pub address: String,
    /// The server's root, for the unversioned routes (`/`, the API docs).
    pub root: String,
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
    spawn(auth, PgPoolOptions::new()).await
}

/// Spawns the control plane, with authentication disabled, on a pool of one
/// database connection, for requests that must not hold one and wait for
/// another: they fail when the pool gives up waiting, after 5 s.
pub async fn spawn_app_on_one_connection() -> TestApp {
    spawn(
        AuthSettings {
            enabled: false,
            tokens: vec![],
        },
        PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5)),
    )
    .await
}

async fn spawn(auth: AuthSettings, app_pool: PgPoolOptions) -> TestApp {
    LazyLock::force(&TRACING);
    let mut configuration = get_configuration().expect("Failed to read configuration");
    configuration.database.name = Uuid::new_v4().to_string();
    let pool = configure_database(&configuration.database).await;

    let listener = StdTcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    listener.set_nonblocking(true).unwrap();
    let root = format!("http://{}", listener.local_addr().unwrap());
    let address = format!("{root}/v1");

    // The app gets its own runtime, pool and listener. If it shared the test's
    // runtime, TestApp::drop (which blocks that runtime) could stall one of its
    // pool connections mid-handshake, and DROP DATABASE would then wait for
    // Postgres' 60 s authentication_timeout.
    let (shutdown, shutdown_rx) = oneshot::channel();
    let database = configuration.database.clone();
    // Loading it installs the process-wide TLS crypto provider, which a test's
    // fake node needs as soon as spawn_app returns.
    let node_tls = NodeTls::load(&NODE_TLS_DIR.path().join("control-plane")).unwrap();
    let handle = std::thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let app_pool = app_pool
                .connect(&database.connection_string())
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
                node_tls,
                UI_DIR.path().to_owned(),
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
        root,
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

/// 127.0.0.1 is a fake node's: the address its certificate names.
static NEXT_HOST: AtomicU8 = AtomicU8::new(2);

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

/// Sets the memory a host has, as its node would report it.
pub async fn set_total_memory(app: &TestApp, host_id: Uuid, bytes: i64) {
    sqlx::query("UPDATE hosts SET total_memory_bytes = $1 WHERE id = $2")
        .bind(bytes)
        .bind(host_id)
        .execute(&app.pool)
        .await
        .unwrap();
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
