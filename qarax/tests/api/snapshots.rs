use crate::support::{TestApp, add_up_host, create_vm, spawn_app};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

/// Create a local storage pool for snapshots.
async fn ensure_storage_pool(client: &reqwest::Client, address: &str) {
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
}

#[tokio::test]
async fn test_list_snapshots_returns_404_for_unknown_vm() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let random_uuid = Uuid::new_v4();

    let res = client
        .get(format!("{}/vms/{}/snapshots", app.address, random_uuid))
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
        .post(format!("{}/vms/{}/snapshots", app.address, random_uuid))
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

/// A VM on an UP host whose node never answers, and the failed snapshot an
/// attempt to snapshot it leaves: (VM, host, snapshot).
async fn vm_with_failed_snapshot(
    app: &TestApp,
    client: &reqwest::Client,
) -> (Uuid, Uuid, serde_json::Value) {
    let host_id = add_up_host(client, &app.address, "test-host").await;
    ensure_storage_pool(client, &app.address).await;

    let vm_id = create_vm(
        client,
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

    let snapshots_url = format!("{}/vms/{}/snapshots", app.address, vm_id);
    let res = client.get(&snapshots_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.json::<serde_json::Value>().await.unwrap(), json!([]));

    // POST /vms/{vm_id}/snapshots — node is not available, should get 500
    let res = client
        .post(format!("{}/vms/{}/snapshots", app.address, vm_id))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let res = client
        .get(format!("{}/vms/{}/snapshots", app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let mut snapshots: Vec<serde_json::Value> = res.json().await.unwrap();
    assert_eq!(snapshots.len(), 1, "Expected 1 snapshot record");
    (vm_id, host_id, snapshots.remove(0))
}

async fn set_host_down(app: &TestApp, host_id: Uuid) {
    sqlx::query("UPDATE hosts SET status = 'DOWN' WHERE id = $1")
        .bind(host_id)
        .execute(&app.pool)
        .await
        .unwrap();
}

async fn storage_object_exists(app: &TestApp, snapshot: &serde_json::Value) -> bool {
    let id: Uuid = snapshot["storage_object_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM storage_objects WHERE id = $1)")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_create_snapshot_with_unavailable_node_creates_failed_record() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (_, _, snapshot) = vm_with_failed_snapshot(&app, &client).await;
    assert_eq!(
        snapshot["status"], "failed",
        "Expected snapshot status 'failed', got: {}",
        snapshot["status"]
    );
}

#[tokio::test]
async fn test_delete_snapshot_status_codes() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (vm_id, host_id, snapshot) = vm_with_failed_snapshot(&app, &client).await;
    let snapshot_id = snapshot["id"].as_str().unwrap();
    let delete = |vm: Uuid, snapshot: &str| {
        client
            .delete(format!("{}/vms/{vm}/snapshots/{snapshot}", app.address))
            .send()
    };

    assert_eq!(
        delete(Uuid::new_v4(), snapshot_id).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        delete(vm_id, &Uuid::new_v4().to_string())
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let other_vm = create_vm(
        &client,
        &app.address,
        json!({
            "name": "test-vm-snap-other",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456
        }),
    )
    .await;
    assert_eq!(
        delete(other_vm, snapshot_id).await.unwrap().status(),
        StatusCode::NOT_FOUND,
        "another VM's snapshot"
    );

    sqlx::query("UPDATE vm_snapshots SET status = 'CREATING' WHERE id = $1::uuid")
        .bind(snapshot_id)
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        delete(vm_id, snapshot_id).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE vm_snapshots SET status = 'FAILED' WHERE id = $1::uuid")
        .bind(snapshot_id)
        .execute(&app.pool)
        .await
        .unwrap();

    // Neither the pool nor the VM has an UP host to remove the data from.
    set_host_down(&app, host_id).await;
    assert_eq!(
        delete(vm_id, snapshot_id).await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(
        storage_object_exists(&app, &snapshot).await,
        "a refused delete keeps the snapshot"
    );
}

#[tokio::test]
async fn test_delete_vm_deletes_its_snapshots() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (vm_id, host_id, snapshot) = vm_with_failed_snapshot(&app, &client).await;
    // With no UP host the files can't be removed; the records go anyway.
    set_host_down(&app, host_id).await;

    let res = client
        .delete(format!("{}/vms/{vm_id}", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(!storage_object_exists(&app, &snapshot).await);
}
