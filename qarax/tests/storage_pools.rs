use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{DatabaseSettings, default_control_plane_architecture, get_configuration},
    model::{
        hosts::{self, HostStatus, NewHost},
        storage_pools::{self, NewStoragePool, StoragePoolType},
    },
    startup::run,
};
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::runtime::Runtime;
use uuid::Uuid;

struct TestApp {
    db_name: String,
    address: String,
    pool: PgPool,
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

async fn configure_database(config: &DatabaseSettings) -> PgPool {
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
    let address = format!("http://127.0.0.1:{port}");

    let mut configuration = get_configuration().expect("Failed to read configuration.");
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

async fn create_test_pool(pool: &PgPool, pool_type: StoragePoolType) -> Uuid {
    let config = match &pool_type {
        StoragePoolType::Local => json!({ "path": format!("/tmp/{}", Uuid::new_v4()) }),
        StoragePoolType::Nfs => json!({
            "server": "127.0.0.1",
            "export_path": format!("/exports/{}", Uuid::new_v4()),
        }),
        StoragePoolType::OverlayBd => json!({ "url": "http://registry:5000" }),
        StoragePoolType::Block => json!({
            "portal": "127.0.0.1:3260",
            "iqn": "iqn.2024-01.qarax:test",
        }),
    };

    storage_pools::create(
        pool,
        NewStoragePool {
            name: format!("test-pool-{}", Uuid::new_v4()),
            pool_type,
            config,
            capacity_bytes: None,
        },
    )
    .await
    .unwrap()
}

async fn create_test_host(pool: &PgPool, status: HostStatus) -> Uuid {
    let host_id = hosts::add(
        pool,
        &NewHost {
            name: format!("test-host-{}", Uuid::new_v4()),
            address: "127.0.0.1".to_string(),
            port: 1,
            host_user: "root".to_string(),
            password: String::new(),
            reservation_class: None,
            placement_labels: std::collections::BTreeMap::new(),
        },
    )
    .await
    .unwrap();

    if status != HostStatus::Down {
        hosts::update_status(pool, host_id, status).await.unwrap();
    }

    host_id
}

async fn attach_host_to_pool(pool: &PgPool, pool_id: Uuid, host_id: Uuid) {
    storage_pools::attach_host(pool, pool_id, host_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn create_disk_accepts_source_backed_requests_without_size_bytes() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let pool_id = create_test_pool(&app.pool, StoragePoolType::Local).await;
    let host_id = create_test_host(&app.pool, HostStatus::Up).await;
    attach_host_to_pool(&app.pool, pool_id, host_id).await;

    let response = client
        .post(format!("{}/storage-pools/{pool_id}/disks", app.address))
        .json(&json!({
            "name": "downloaded-disk",
            "source_url": "http://127.0.0.1:1/disk.img",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body: serde_json::Value = response.json().await.unwrap();
    Uuid::parse_str(body["storage_object_id"].as_str().unwrap()).unwrap();
    Uuid::parse_str(body["job_id"].as_str().unwrap()).unwrap();
}

#[tokio::test]
async fn create_disk_requires_positive_size_for_blank_disks() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let pool_id = create_test_pool(&app.pool, StoragePoolType::Local).await;

    let missing_size = client
        .post(format!("{}/storage-pools/{pool_id}/disks", app.address))
        .json(&json!({
            "name": "blank-disk",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(missing_size.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let missing_body: serde_json::Value = missing_size.json().await.unwrap();
    assert_eq!(
        missing_body["message"],
        "size_bytes is required when source_url is not provided"
    );

    let zero_size = client
        .post(format!("{}/storage-pools/{pool_id}/disks", app.address))
        .json(&json!({
            "name": "blank-disk",
            "size_bytes": 0,
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(zero_size.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let zero_body: serde_json::Value = zero_size.json().await.unwrap();
    assert_eq!(zero_body["message"], "size_bytes must be greater than 0");
}

#[tokio::test]
async fn create_disk_requires_an_up_host_attached_to_the_pool() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let pool_id = create_test_pool(&app.pool, StoragePoolType::Local).await;
    let down_host_id = create_test_host(&app.pool, HostStatus::Down).await;
    attach_host_to_pool(&app.pool, pool_id, down_host_id).await;

    let response = client
        .post(format!("{}/storage-pools/{pool_id}/disks", app.address))
        .json(&json!({
            "name": "blank-disk",
            "size_bytes": 1024,
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["message"], "No UP host attached to this storage pool");
}

#[tokio::test]
async fn import_to_pool_requires_an_up_host_attached_to_the_pool() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let pool_id = create_test_pool(&app.pool, StoragePoolType::OverlayBd).await;
    let down_host_id = create_test_host(&app.pool, HostStatus::Down).await;
    attach_host_to_pool(&app.pool, pool_id, down_host_id).await;

    let response = client
        .post(format!("{}/storage-pools/{pool_id}/import", app.address))
        .json(&json!({
            "name": "imported-image",
            "image_ref": "registry:5000/test/image:latest",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["message"], "No UP host attached to this storage pool");
}
