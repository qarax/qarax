mod support;

use qarax::model::{
    hosts::{self, HostStatus, NewHost},
    storage_objects,
    storage_pools::{self, NewStoragePool, StoragePoolType},
};
use reqwest::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use std::assert_matches;
use support::spawn_app;
use uuid::Uuid;

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
            credential_ref: None,
            reservation_class: None,
            placement_labels: std::collections::BTreeMap::new(),
        },
    )
    .await
    .unwrap();

    hosts::transition(pool, host_id, &[HostStatus::Down], status)
        .await
        .unwrap();

    host_id
}

async fn attach_host_to_pool(pool: &PgPool, pool_id: Uuid, host_id: Uuid) {
    storage_pools::attach_host(pool, pool_id, host_id)
        .await
        .unwrap();
}

/// Resizes through the transaction-scoped writer the resize handler uses.
async fn set_size_bytes(pool: &PgPool, id: Uuid, size_bytes: i64) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    storage_objects::update_size_bytes_tx(&mut tx, id, size_bytes).await?;
    tx.commit().await
}

#[tokio::test]
async fn pool_capacity_tracks_concurrent_creates_resizes_and_deletes() {
    use qarax::model::storage_objects::{self, NewStorageObject, StorageObjectType};

    let app = spawn_app().await;
    let pool_id = storage_pools::create(
        &app.pool,
        NewStoragePool {
            name: "limited-pool".into(),
            pool_type: StoragePoolType::Local,
            config: json!({"path": "/tmp/capacity-test"}),
            capacity_bytes: Some(10),
        },
    )
    .await
    .unwrap();
    let object = |name: &str| NewStorageObject {
        name: name.into(),
        storage_pool_id: Some(pool_id),
        object_type: StorageObjectType::Disk,
        size_bytes: 6,
        config: json!({}),
        parent_id: None,
    };
    let (a, b) = tokio::join!(
        storage_objects::create(&app.pool, object("a")),
        storage_objects::create(&app.pool, object("b")),
    );
    let (id, error) = match (a, b) {
        (Ok(id), Err(error)) | (Err(error), Ok(id)) => (id, error),
        other => panic!("exactly one reservation must succeed: {other:?}"),
    };
    assert_matches!(
        qarax::errors::Error::from(error),
        qarax::errors::Error::UnprocessableEntity(_)
    );
    assert_eq!(
        storage_pools::get(&app.pool, pool_id)
            .await
            .unwrap()
            .allocated_bytes,
        Some(6)
    );
    assert!(set_size_bytes(&app.pool, id, 11).await.is_err());
    assert_eq!(
        storage_objects::get(&app.pool, id)
            .await
            .unwrap()
            .size_bytes,
        6
    );
    set_size_bytes(&app.pool, id, 8).await.unwrap();
    assert!(set_size_bytes(&app.pool, id, -1).await.is_err());
    assert_eq!(
        storage_pools::get(&app.pool, pool_id)
            .await
            .unwrap()
            .allocated_bytes,
        Some(8)
    );
    storage_objects::delete(&app.pool, id).await.unwrap();
    assert_eq!(
        storage_pools::get(&app.pool, pool_id)
            .await
            .unwrap()
            .allocated_bytes,
        Some(0)
    );

    let response = reqwest::Client::new().post(format!("{}/storage-objects", app.address))
        .json(&json!({"name": "too-large", "storage_pool_id": pool_id, "object_type": "disk", "size_bytes": 11}))
        .send().await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.text().await.unwrap().contains("capacity"));
}

#[tokio::test]
async fn disk_resize_reserves_capacity_before_rpc_and_rolls_back_on_failure() {
    use qarax::model::storage_objects::{self, NewStorageObject, StorageObjectType};
    const MIB: i64 = 1024 * 1024;
    let app = spawn_app().await;
    let pool_id = storage_pools::create(
        &app.pool,
        NewStoragePool {
            name: "resize-pool".into(),
            pool_type: StoragePoolType::Local,
            config: json!({"path": "/tmp/resize-capacity-test"}),
            capacity_bytes: Some(2 * MIB),
        },
    )
    .await
    .unwrap();
    let host_id = create_test_host(&app.pool, HostStatus::Up).await;
    attach_host_to_pool(&app.pool, pool_id, host_id).await;
    let object_id = storage_objects::create(
        &app.pool,
        NewStorageObject {
            name: "disk".into(),
            storage_pool_id: Some(pool_id),
            object_type: StorageObjectType::Disk,
            size_bytes: MIB,
            config: json!({}),
            parent_id: None,
        },
    )
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/vms", app.address))
        .json(&json!({
            "name": "resize-vm", "hypervisor": "cloud_hv", "boot_vcpus": 1, "max_vcpus": 1,
            "memory_size": 268435456, "root_disk_object_id": object_id,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let vm_id: String = response.json().await.unwrap();
    let url = format!("{}/vms/{vm_id}/disks/rootfs/resize", app.address);
    let response = client
        .put(&url)
        .json(&json!({"new_size_bytes": 3 * MIB}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.text().await.unwrap().contains("capacity"));

    // The host's gRPC port is deliberately unreachable: no filesystem changed.
    let response = client
        .put(url)
        .json(&json!({"new_size_bytes": 2 * MIB}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        storage_objects::get(&app.pool, object_id)
            .await
            .unwrap()
            .size_bytes,
        MIB
    );
    assert_eq!(
        storage_pools::get(&app.pool, pool_id)
            .await
            .unwrap()
            .allocated_bytes,
        Some(MIB)
    );
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
