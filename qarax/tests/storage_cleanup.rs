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
        configuration.database.clone(),
        configuration.vm_defaults.clone(),
        configuration.scheduling.clone(),
        configuration.sandbox.clone(),
        configuration.auth.clone(),
        configuration.ha.clone(),
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

#[tokio::test]
async fn deleting_attached_persistent_upper_preserves_the_reference() {
    let app = spawn_app().await;
    let pool_id = create_test_pool(&app.pool).await;
    let mut object_ids = Vec::new();
    for object_type in [
        StorageObjectType::OciImage,
        StorageObjectType::OverlaybdUpper,
    ] {
        object_ids.push(
            storage_objects::create(
                &app.pool,
                NewStorageObject {
                    name: object_type.to_string(),
                    storage_pool_id: Some(pool_id),
                    object_type,
                    size_bytes: 0,
                    config: json!({}),
                    parent_id: None,
                },
            )
            .await
            .unwrap(),
        );
    }
    let vm_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO vms (id, name, status, hypervisor, config, boot_vcpus, max_vcpus, memory_size)
         VALUES ($1, 'persistent-vm', 'CREATED', 'CLOUD_HV', '{}', 1, 1, 268435456)",
    )
    .bind(vm_id)
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO vm_disks (vm_id, storage_object_id, upper_storage_object_id, logical_name, device_path)
         VALUES ($1, $2, $3, 'rootfs', '/dev/vda')",
    )
    .bind(vm_id)
    .bind(object_ids[0])
    .bind(object_ids[1])
    .execute(&app.pool)
    .await
    .unwrap();

    let client = reqwest::Client::new();
    let url = format!("{}/storage-objects/{}", app.address, object_ids[1]);
    let response = client.delete(&url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("persistent VM write layer")
    );
    let disks = qarax::model::vm_disks::list_by_vm(&app.pool, vm_id)
        .await
        .unwrap();
    assert_eq!(disks[0].upper_storage_object_id, Some(object_ids[1]));
    storage_objects::get(&app.pool, object_ids[1])
        .await
        .unwrap();

    qarax::model::vm_disks::delete_by_vm(&app.pool, vm_id)
        .await
        .unwrap();
    let response = client.delete(url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[derive(Clone)]
struct CleanupNode {
    unavailable: std::sync::Arc<std::sync::atomic::AtomicBool>,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[tonic::async_trait]
impl qarax::grpc_client::node::file_transfer_service_server::FileTransferService for CleanupNode {
    async fn delete_storage_files(
        &self,
        request: tonic::Request<qarax::grpc_client::node::DeleteStorageFilesRequest>,
    ) -> Result<tonic::Response<qarax::grpc_client::node::DeleteStorageFilesResponse>, tonic::Status>
    {
        use std::sync::atomic::Ordering;
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(tonic::Status::unavailable("test outage"));
        }
        let req = request.into_inner();
        let config: serde_json::Value = serde_json::from_str(&req.config_json).unwrap();
        let path = std::path::Path::new(config["path"].as_str().unwrap()).join(req.object_id);
        tokio::fs::remove_file(path).await.unwrap();
        Ok(tonic::Response::new(
            qarax::grpc_client::node::DeleteStorageFilesResponse {},
        ))
    }

    async fn download_file(
        &self,
        _: tonic::Request<qarax::grpc_client::node::DownloadFileRequest>,
    ) -> Result<tonic::Response<qarax::grpc_client::node::TransferResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("unused"))
    }
    async fn copy_file(
        &self,
        _: tonic::Request<qarax::grpc_client::node::CopyFileRequest>,
    ) -> Result<tonic::Response<qarax::grpc_client::node::TransferResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("unused"))
    }
    type CreateDiskStream =
        futures::stream::Empty<Result<qarax::grpc_client::node::TransferResponse, tonic::Status>>;
    async fn create_disk(
        &self,
        _: tonic::Request<qarax::grpc_client::node::CreateDiskRequest>,
    ) -> Result<tonic::Response<Self::CreateDiskStream>, tonic::Status> {
        Err(tonic::Status::unimplemented("unused"))
    }
}

#[tokio::test]
async fn managed_disk_cleanup_retries_without_losing_allocation_or_deleting_registered_files() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    let app = spawn_app().await;
    let directory = tempfile::tempdir().unwrap();
    let pool_id = storage_pools::create(
        &app.pool,
        NewStoragePool {
            name: "managed".into(),
            pool_type: StoragePoolType::Local,
            config: json!({"path": directory.path()}),
            capacity_bytes: Some(64),
        },
    )
    .await
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let node = CleanupNode {
        unavailable: Arc::new(AtomicBool::new(true)),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let incoming = async_stream::stream! {
        loop { yield listener.accept().await.map(|(stream, _)| stream); }
    };
    let server = tokio::spawn(tonic::transport::Server::builder()
        .add_service(qarax::grpc_client::node::file_transfer_service_server::FileTransferServiceServer::new(node.clone()))
        .serve_with_incoming(incoming));
    let host_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosts(id, name, address, port, host_user, status) VALUES ($1, 'cleanup-node', '127.0.0.1', $2, 'root', 'DOWN')")
        .bind(host_id).bind(i32::from(port)).execute(&app.pool).await.unwrap();
    storage_pools::attach_host(&app.pool, pool_id, host_id)
        .await
        .unwrap();
    let disk = storage_objects::create_managed_returning(
        &app.pool,
        NewStorageObject {
            name: "owned".into(),
            storage_pool_id: Some(pool_id),
            object_type: StorageObjectType::Disk,
            size_bytes: 64,
            config: json!({}),
            parent_id: None,
        },
        host_id,
    )
    .await
    .unwrap();
    let path = disk.config["path"].as_str().unwrap();
    tokio::fs::write(path, b"owned").await.unwrap();
    let client = reqwest::Client::new();

    // Registering the same path does not grant permission to delete its file.
    let registration = storage_objects::create(
        &app.pool,
        NewStorageObject {
            name: "external".into(),
            storage_pool_id: Some(pool_id),
            object_type: StorageObjectType::Disk,
            size_bytes: 0,
            config: disk.config.clone(),
            parent_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        client
            .delete(format!("{}/storage-objects/{registration}", app.address))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(std::path::Path::new(path).exists());
    assert_eq!(node.calls.load(Ordering::SeqCst), 0);

    // An attached managed disk must fail before making any destructive RPC.
    let vm_id = Uuid::new_v4();
    sqlx::query("INSERT INTO vms(id, name, status, hypervisor, config, boot_vcpus, max_vcpus, memory_size) VALUES ($1, 'cleanup-vm', 'CREATED', 'CLOUD_HV', '{}', 1, 1, 268435456)")
        .bind(vm_id).execute(&app.pool).await.unwrap();
    sqlx::query("INSERT INTO vm_disks(vm_id, storage_object_id, logical_name, device_path) VALUES ($1, $2, 'rootfs', '/dev/vda')")
        .bind(vm_id).bind(disk.id).execute(&app.pool).await.unwrap();
    let url = format!("{}/storage-objects/{}", app.address, disk.id);
    assert_eq!(
        client.delete(&url).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(node.calls.load(Ordering::SeqCst), 0);
    qarax::model::vm_disks::delete_by_vm(&app.pool, vm_id)
        .await
        .unwrap();

    assert_eq!(
        client.delete(&url).send().await.unwrap().status(),
        StatusCode::ACCEPTED
    );
    assert!(matches!(
        storage_objects::get(&app.pool, disk.id).await,
        Err(sqlx::Error::RowNotFound)
    ));
    assert_eq!(
        storage_pools::get(&app.pool, pool_id)
            .await
            .unwrap()
            .allocated_bytes,
        Some(64)
    );
    assert!(std::path::Path::new(path).exists());
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM storage_cleanup WHERE object_id = $1")
            .bind(disk.id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(attempts > 0);
    assert_eq!(
        client
            .delete(format!("{}/storage-pools/{pool_id}", app.address))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );

    node.unavailable.store(false, Ordering::SeqCst);
    // Repeating DELETE retries the durable task even though the object is gone.
    assert_eq!(
        client.delete(&url).send().await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert!(!std::path::Path::new(path).exists());
    assert_eq!(
        storage_pools::get(&app.pool, pool_id)
            .await
            .unwrap()
            .allocated_bytes,
        Some(0)
    );
    assert_eq!(
        client.delete(&url).send().await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .delete(format!("{}/storage-pools/{pool_id}", app.address))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    server.abort();
}
