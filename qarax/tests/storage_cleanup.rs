use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{DatabaseSettings, default_control_plane_architecture, get_configuration},
    model::{
        storage_objects::{self, NewStorageObject, StorageObjectType},
        storage_pools::{self, NewStoragePool, StoragePoolType},
        transfers::{self, NewTransfer, TransferType},
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
    let address = format!("http://127.0.0.1:{}", port);

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

async fn create_test_pool(pool: &PgPool) -> Uuid {
    storage_pools::create(
        pool,
        NewStoragePool {
            name: format!("test-pool-{}", Uuid::new_v4()),
            pool_type: StoragePoolType::Local,
            config: json!({ "path": format!("/tmp/{}", Uuid::new_v4()) }),
            capacity_bytes: None,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn deleting_completed_transfer_artifacts_via_api_succeeds() {
    let app = spawn_app().await;
    let pool_id = create_test_pool(&app.pool).await;

    let transfer = transfers::create(
        &app.pool,
        pool_id,
        &NewTransfer {
            name: format!("transfer-{}", Uuid::new_v4()),
            source: "/tmp/source.img".to_string(),
            object_type: StorageObjectType::Kernel,
        },
        TransferType::LocalCopy,
    )
    .await
    .unwrap();

    let object_id = storage_objects::create(
        &app.pool,
        NewStorageObject {
            name: format!("object-{}", Uuid::new_v4()),
            storage_pool_id: Some(pool_id),
            object_type: StorageObjectType::Kernel,
            size_bytes: 64,
            config: json!({}),
            parent_id: None,
        },
    )
    .await
    .unwrap();

    transfers::mark_completed(&app.pool, transfer.id, object_id, 64)
        .await
        .unwrap();

    let client = reqwest::Client::new();

    let delete_object = client
        .delete(format!("{}/storage-objects/{}", app.address, object_id))
        .send()
        .await
        .unwrap();
    assert_eq!(delete_object.status(), StatusCode::NO_CONTENT);

    let transfer_after_object_delete = transfers::get(&app.pool, transfer.id).await.unwrap();
    assert_eq!(transfer_after_object_delete.storage_object_id, None);

    let delete_pool = client
        .delete(format!("{}/storage-pools/{}", app.address, pool_id))
        .send()
        .await
        .unwrap();
    assert_eq!(delete_pool.status(), StatusCode::NO_CONTENT);

    let err = transfers::get(&app.pool, transfer.id).await.unwrap_err();
    assert!(matches!(err, sqlx::Error::RowNotFound));
}
