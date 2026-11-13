mod support;

use qarax::model::storage_pools::{self, NewStoragePool, StoragePoolType};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{add_host, add_up_host, create_vm, set_vm_status, spawn_app};
use uuid::Uuid;

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
    let host_id = add_host(&client, &app.address, host_name).await;
    add_up_host(&client, &app.address, "audit-other-host").await;
    create_vm(&client, &app.address, json!({
        "name": "audit-vm", "hypervisor": "cloud_hv", "boot_vcpus": 1,
        "max_vcpus": 1, "memory_size": 268435456, "config": {}
    }))
    .await;
    let list = |query: String| {
        let client = client.clone();
        let url = format!("{}/audit-logs?{query}", app.address);
        async move {
            let res = client.get(url).send().await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            res.json::<Vec<Value>>().await.unwrap()
        }
    };

    let logs = list(format!("resource_type=host&resource_id={host_id}&action=create")).await;
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["resource_id"], host_id.to_string());
    assert_eq!(logs[0]["resource_name"], host_name);
    assert!(logs[0]["created_at"].is_string());

    // Each filter narrows the result: two hosts and one VM were created.
    assert_eq!(list("resource_type=host&action=create".into()).await.len(), 2);
    assert_eq!(list("action=create".into()).await.len(), 3);
    assert!(list("resource_type=host&action=delete".into()).await.is_empty());
    assert_eq!(list("limit=1".into()).await.len(), 1);

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

    let res = client
        .get(format!("{}/audit-logs/{}", app.address, Uuid::nil()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
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

    let _ = add_host(&client, &app.address, host_name).await;

    let res = client
        .post(format!("{}/hosts", app.address))
        .json(&json!({
            "name": host_name,
            "address": "127.0.0.1",
            "port": 50052,
            "host_user": "root"
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
async fn test_rejected_vm_start_is_not_audited() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "start-host").await;
    let vm_id = create_vm(&client, &app.address, json!({
        "name": "audit-running-vm", "hypervisor": "cloud_hv", "boot_vcpus": 1,
        "max_vcpus": 1, "memory_size": 268435456, "config": {}
    }))
    .await;
    set_vm_status(&app.pool, vm_id, "RUNNING").await;

    let res = client
        .post(format!("{}/vms/{vm_id}/start", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);

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
    let host_id = add_up_host(&client, &app.address, "overlaybd-host").await;
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
