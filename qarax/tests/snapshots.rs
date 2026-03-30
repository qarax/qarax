use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{DatabaseSettings, default_control_plane_architecture, get_configuration},
    startup::run,
};
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::runtime::Runtime;
use uuid::Uuid;

struct TestApp {
    pub db_name: String,
    pub address: String,
    pub pool: PgPool,
}

static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_subscriber(subscriber);
    } else {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_subscriber(subscriber);
    }
});

pub async fn configure_database(config: &DatabaseSettings) -> PgPool {
    let mut connection = PgConnection::connect(&config.connection_string_without_db())
        .await
        .expect("Failed to connect to Postgres");
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.name).as_str())
        .await
        .expect("Failed to create database.");
    let connection_pool = PgPool::connect(&config.connection_string())
        .await
        .expect("Failed to connect to Postgres.");
    sqlx::migrate!("../migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");
    connection_pool
}

async fn spawn_app() -> TestApp {
    Lazy::force(&TRACING);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{}", port);
    let mut configuration =
        qarax::configuration::get_configuration().expect("Failed to read configuration.");
    configuration.database.name = Uuid::new_v4().to_string();
    let connection_pool = configure_database(&configuration.database).await;

    let server = run(
        listener,
        connection_pool.clone(),
        configuration.vm_defaults.clone(),
        configuration.scheduling.clone(),
        default_control_plane_architecture(),
    )
    .await
    .unwrap();
    std::thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        let _ = rt.block_on(async move { server.await });
    });
    TestApp {
        db_name: configuration.database.name,
        address,
        pool: connection_pool,
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let db_name = self.db_name.clone();
        std::thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async {
                let config = get_configuration().expect("Failed to read configuration");
                let mut conn = PgConnection::connect_with(&config.database.without_db())
                    .await
                    .expect("Failed to connect to Postgres");
                conn.execute(&*format!("DROP DATABASE \"{}\" WITH (FORCE)", db_name))
                    .await
                    .expect("Failed to drop database.");
                let _ = tx.send(());
            })
        });
        let _ = rx.recv();
    }
}

/// Create a host and set it to UP so the scheduler can assign VMs.
async fn ensure_host_up(client: &reqwest::Client, address: &str) -> String {
    let res = client
        .post(format!("{}/hosts", address))
        .json(&json!({
            "name": "test-host",
            "address": "127.0.0.1",
            "port": 50051,
            "host_user": "root",
            "password": ""
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let host_id = res.text().await.unwrap();

    client
        .patch(format!("{}/hosts/{}", address, host_id))
        .json(&json!({"status": "up"}))
        .send()
        .await
        .unwrap();

    host_id
}

/// Create a local storage pool and return its UUID string.
async fn ensure_storage_pool(client: &reqwest::Client, address: &str) -> String {
    let res = client
        .post(format!("{}/storage-pools", address))
        .json(&json!({
            "name": "test-pool",
            "pool_type": "local",
            "config": { "path": "/tmp/test-pool" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::CREATED,
        "Storage pool creation failed"
    );
    res.text().await.unwrap()
}

/// Create a VM via the API, returns the VM UUID string.
async fn create_vm(client: &reqwest::Client, address: &str, body: serde_json::Value) -> String {
    let res = client
        .post(format!("{}/vms", address))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED, "VM creation failed");
    res.json().await.unwrap()
}

#[tokio::test]
async fn test_list_snapshots_returns_empty_for_vm_without_snapshots() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "test-vm-snap-empty",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    let res = client
        .get(format!("{}/vms/{}/snapshots", &app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn test_list_snapshots_returns_404_for_unknown_vm() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let random_uuid = Uuid::new_v4();

    let res = client
        .get(format!("{}/vms/{}/snapshots", &app.address, random_uuid))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body.get("message").is_some(),
        "Expected 'message' field in 404 response, got: {}",
        body
    );
}

#[tokio::test]
async fn test_create_snapshot_returns_404_for_unknown_vm() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let random_uuid = Uuid::new_v4();

    let res = client
        .post(format!("{}/vms/{}/snapshots", &app.address, random_uuid))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body.get("message").is_some(),
        "Expected 'message' field in 404 response, got: {}",
        body
    );
}

#[tokio::test]
async fn test_create_snapshot_with_unavailable_node_creates_failed_record() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;
    ensure_storage_pool(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "test-vm-snap-fail",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    // POST /vms/{vm_id}/snapshots — node is not available, should get 500
    let res = client
        .post(format!("{}/vms/{}/snapshots", &app.address, vm_id))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // GET /vms/{vm_id}/snapshots — should have 1 snapshot with status "failed"
    let res = client
        .get(format!("{}/vms/{}/snapshots", &app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let snapshots: Vec<serde_json::Value> = res.json().await.unwrap();
    assert_eq!(snapshots.len(), 1, "Expected 1 snapshot record");
    assert_eq!(
        snapshots[0]["status"], "failed",
        "Expected snapshot status 'failed', got: {}",
        snapshots[0]["status"]
    );
}
