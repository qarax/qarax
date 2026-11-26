use crate::support::fake_node::{SNAPSHOT_REFUSAL, add_forking_node};
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

/// A snapshot its node refuses, as it refuses a VM with a disk it can't
/// capture, is the request's to fix: 422 with the node's reason, and a failed
/// snapshot record.
#[tokio::test]
async fn test_a_snapshot_the_node_refuses_is_422_with_its_reason() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (_, _node) = add_forking_node(&app, "snapshot-node").await;
    ensure_storage_pool(&client, &app.address).await;
    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "test-vm-snap-refused",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    let res = client
        .post(format!("{}/vms/{}/snapshots", app.address, vm_id))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["message"], SNAPSHOT_REFUSAL, "{body}");

    let snapshots: serde_json::Value = client
        .get(format!("{}/vms/{}/snapshots", app.address, vm_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(snapshots[0]["status"], "failed", "{snapshots}");
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

/// A backup copy is refused, before any job, for what it can't do: copy a
/// backup that isn't there, isn't a VM's or isn't ready, or copy to a pool
/// that isn't there, isn't Local or NFS, is the backup's own, or has no UP
/// host to write the copy.
#[tokio::test]
async fn test_backup_copy_refusals() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (vm_id, _, snapshot) = vm_with_failed_snapshot(&app, &client).await;
    let snapshot_id: Uuid = snapshot["id"].as_str().unwrap().parse().unwrap();
    let object_id: Uuid = snapshot["storage_object_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    sqlx::query("UPDATE vm_snapshots SET status = 'READY' WHERE id = $1")
        .bind(snapshot_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let backup_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backups (id, name, backup_type, status, vm_id, snapshot_id, storage_object_id) \
         VALUES ($1, 'backup', 'VM', 'READY', $2, $3, $4)",
    )
    .bind(backup_id)
    .bind(vm_id)
    .bind(snapshot_id)
    .bind(object_id)
    .execute(&app.pool)
    .await
    .unwrap();
    let source_pool: Uuid =
        sqlx::query_scalar("SELECT storage_pool_id FROM storage_objects WHERE id = $1")
            .bind(object_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let create_pool = |body: serde_json::Value| {
        let client = client.clone();
        let address = app.address.clone();
        async move {
            let res = client
                .post(format!("{address}/storage-pools"))
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::CREATED);
            res.text().await.unwrap().parse::<Uuid>().unwrap()
        }
    };
    let block = create_pool(json!({
        "name": "copy-block",
        "pool_type": "block",
        "config": {"portal": "10.0.0.1:3260", "iqn": "iqn.2024-01.qarax:copy"}
    }))
    .await;
    let unattached = create_pool(json!({
        "name": "copy-local",
        "pool_type": "local",
        "config": {"path": "/tmp/copy-local"}
    }))
    .await;
    let copy = |backup: Uuid, pool: Uuid| {
        client
            .post(format!("{}/backups/{backup}/copy", app.address))
            .json(&json!({ "storage_pool_id": pool }))
            .send()
    };

    let res = copy(Uuid::new_v4(), unattached).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "a backup that isn't there"
    );
    let res = copy(backup_id, Uuid::new_v4()).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "a pool that isn't there"
    );
    for (pool, why) in [
        (source_pool, "already"),
        (block, "Local or NFS"),
        (unattached, "no UP host"),
    ] {
        let res = copy(backup_id, pool).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{why}");
        let body: serde_json::Value = res.json().await.unwrap();
        assert!(body["message"].as_str().unwrap().contains(why), "{body}");
    }

    sqlx::query("UPDATE backups SET status = 'CREATING' WHERE id = $1")
        .bind(backup_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let res = copy(backup_id, unattached).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "not ready");

    let dump_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO storage_objects (id, name, storage_pool_id, object_type, size_bytes, config) \
         VALUES ($1, 'dump', $2, 'DATABASE_BACKUP', 0, '{}')",
    )
    .bind(dump_id)
    .bind(source_pool)
    .execute(&app.pool)
    .await
    .unwrap();
    let database_backup = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backups (id, name, backup_type, status, storage_object_id) \
         VALUES ($1, 'dump', 'DATABASE', 'READY', $2)",
    )
    .bind(database_backup)
    .bind(dump_id)
    .execute(&app.pool)
    .await
    .unwrap();
    let res = copy(database_backup, unattached).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "a database backup"
    );

    let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE job_type = 'BACKUP_COPY'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(jobs, 0, "a refused copy starts no job");
}
