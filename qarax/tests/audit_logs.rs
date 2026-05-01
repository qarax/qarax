use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{DatabaseSettings, default_control_plane_architecture, get_configuration},
    model::storage_pools::{self, NewStoragePool, StoragePoolType},
    startup::run,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
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
        configuration.database.clone(),
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

async fn add_host(client: &reqwest::Client, address: &str, name: &str, port: u16) -> Uuid {
    let res = client
        .post(format!("{address}/hosts"))
        .json(&json!({
            "name": name,
            "address": "127.0.0.1",
            "port": port,
            "host_user": "root",
            "password": ""
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    Uuid::parse_str(&res.text().await.unwrap()).unwrap()
}

async fn add_up_host(client: &reqwest::Client, address: &str, name: &str, port: u16) -> Uuid {
    let host_id = add_host(client, address, name, port).await;
    let res = client
        .patch(format!("{address}/hosts/{host_id}"))
        .json(&json!({"status": "up"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    host_id
}

async fn add_overlaybd_pool(pool: &PgPool, host_id: Uuid) {
    let pool_id = storage_pools::create(
        pool,
        NewStoragePool {
            name: format!("overlaybd-{host_id}"),
            pool_type: StoragePoolType::OverlayBd,
            config: json!({ "url": "http://registry:5000" }),
            capacity_bytes: None,
        },
    )
    .await
    .unwrap();

    storage_pools::attach_host(pool, pool_id, host_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_host_create_is_recorded_in_audit_log() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_name = "audit-host";
    let host_id = add_host(&client, &app.address, host_name, 50051).await;

    let res = client
        .get(format!(
            "{}/audit-logs?resource_type=host&resource_id={host_id}&action=create",
            app.address
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let logs: Vec<Value> = res.json().await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["resource_id"], host_id.to_string());
    assert_eq!(logs[0]["resource_name"], host_name);

    let log_id = logs[0]["id"].as_str().unwrap();
    let res = client
        .get(format!("{}/audit-logs/{log_id}", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let log: Value = res.json().await.unwrap();
    assert_eq!(log["id"], log_id);
    assert_eq!(log["resource_type"], "host");
    assert_eq!(log["action"], "create");
}

#[tokio::test]
async fn test_invalid_audit_log_action_filter_is_rejected() {
    let app = spawn_app().await;

    let res = reqwest::get(format!(
        "{}/audit-logs?action=not-a-real-action",
        app.address
    ))
    .await
    .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_failed_host_create_does_not_record_audit_log() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_name = "duplicate-audit-host";

    let _ = add_host(&client, &app.address, host_name, 50051).await;

    let res = client
        .post(format!("{}/hosts", app.address))
        .json(&json!({
            "name": host_name,
            "address": "127.0.0.1",
            "port": 50052,
            "host_user": "root",
            "password": ""
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let res = client
        .get(format!(
            "{}/audit-logs?resource_type=host&action=create",
            app.address
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let logs: Vec<Value> = res.json().await.unwrap();

    let duplicates = logs
        .iter()
        .filter(|log| log["resource_name"] == host_name)
        .count();
    assert_eq!(duplicates, 1);
}

#[tokio::test]
async fn test_failed_vm_start_does_not_record_audit_log() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let vm_id = Uuid::new_v4();

    let res = client
        .post(format!("{}/vms/{vm_id}/start", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = client
        .get(format!(
            "{}/audit-logs?resource_type=vm&resource_id={vm_id}&action=start",
            app.address
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let logs: Vec<Value> = res.json().await.unwrap();
    assert!(logs.is_empty());
}

#[tokio::test]
async fn test_oci_vm_create_is_recorded_in_audit_log() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "overlaybd-host", 9).await;
    add_overlaybd_pool(&app.pool, host_id).await;

    let res = client
        .post(format!("{}/vms", app.address))
        .json(&json!({
            "name": "audit-oci-vm",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "image_ref": "registry:5000/test/busybox:latest",
            "config": {}
        }))
        .send()
        .await
        .unwrap();
    let status = res.status();
    let body = res.text().await.unwrap();
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    let vm_id = body["vm_id"].as_str().unwrap();

    let res = client
        .get(format!(
            "{}/audit-logs?resource_type=vm&resource_id={vm_id}&action=create",
            app.address
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let logs: Vec<Value> = res.json().await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["resource_id"], vm_id);
    assert_eq!(logs[0]["resource_name"], "audit-oci-vm");
}
