mod support;

use reqwest::StatusCode;
use serde_json::json;
use support::{add_up_host, create_vm, spawn_app};
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
    add_up_host(&client, &app.address, "test-host").await;
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

    let snapshots_url = format!("{}/vms/{}/snapshots", &app.address, vm_id);
    let res = client.get(&snapshots_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.json::<serde_json::Value>().await.unwrap(), json!([]));

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
