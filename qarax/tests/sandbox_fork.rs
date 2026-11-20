//! Forking a sandbox, short of the node: what is refused before any node
//! call, and that a fork whose node call fails leaves no rows behind. The fork
//! itself needs a real node; it is in e2e/test_sandboxes.py.

mod support;

use reqwest::StatusCode;
use serde_json::{Value, json};
use support::{TestApp, add_up_host, create_vm, set_vm_status, spawn_app};
use uuid::Uuid;

const MIB: i64 = 1024 * 1024;

/// A READY sandbox whose VM runs, as the node would leave one. `extra` goes
/// into the VM's create body.
async fn running_sandbox(
    app: &TestApp,
    client: &reqwest::Client,
    name: &str,
    hypervisor: &str,
    extra: Value,
) -> (Uuid, Uuid) {
    let mut body = json!({
        "name": name,
        "hypervisor": hypervisor,
        "boot_vcpus": 1,
        "max_vcpus": 1,
        "memory_size": 256 * MIB,
        "guest_agent": true,
        "config": {}
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    let vm_id = create_vm(client, &app.address, body).await;
    set_vm_status(&app.pool, vm_id, "RUNNING").await;
    let sandbox_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sandboxes (id, vm_id, name, status, idle_timeout_secs) \
         VALUES ($1, $2, $3, 'READY', 300)",
    )
    .bind(sandbox_id)
    .bind(vm_id)
    .bind(name)
    .execute(&app.pool)
    .await
    .unwrap();
    (sandbox_id, vm_id)
}

async fn fork(
    app: &TestApp,
    client: &reqwest::Client,
    sandbox_id: Uuid,
    body: Value,
) -> (StatusCode, String) {
    let res = client
        .post(format!("{}/sandboxes/{sandbox_id}/fork", app.address))
        .json(&body)
        .send()
        .await
        .unwrap();
    (res.status(), res.text().await.unwrap())
}

async fn assert_refused(app: &TestApp, client: &reqwest::Client, sandbox_id: Uuid, because: &str) {
    let (status, body) = fork(app, client, sandbox_id, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains(because), "expected {because:?} in {body}");
}

async fn count(app: &TestApp, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn forking_a_missing_sandbox_is_404() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (status, _) = fork(&app, &client, Uuid::new_v4(), json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_fork_count_outside_1_to_16_is_refused() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "fork-host").await;
    let (sandbox_id, _) =
        running_sandbox(&app, &client, "fork-count", "firecracker", json!({})).await;

    for count in [0, 17] {
        let (status, body) = fork(&app, &client, sandbox_id, json!({ "count": count })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{count}: {body}");
        assert!(body.contains("between 1 and 16"), "{count}: {body}");
    }
}

#[tokio::test]
async fn only_a_running_firecracker_sandbox_with_only_memory_to_share_forks() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "fork-host").await;

    let (provisioning, _) =
        running_sandbox(&app, &client, "fork-provisioning", "firecracker", json!({})).await;
    sqlx::query("UPDATE sandboxes SET status = 'PROVISIONING' WHERE id = $1")
        .bind(provisioning)
        .execute(&app.pool)
        .await
        .unwrap();
    assert_refused(&app, &client, provisioning, "is not ready").await;

    let (stopped, stopped_vm) =
        running_sandbox(&app, &client, "fork-stopped", "firecracker", json!({})).await;
    set_vm_status(&app.pool, stopped_vm, "SHUTDOWN").await;
    assert_refused(&app, &client, stopped, "is not running").await;

    let (cloud_hv, _) =
        running_sandbox(&app, &client, "fork-cloud-hv", "cloud_hv", json!({})).await;
    assert_refused(&app, &client, cloud_hv, "only Firecracker").await;

    let (no_agent, _) = running_sandbox(
        &app,
        &client,
        "fork-no-agent",
        "firecracker",
        json!({ "guest_agent": false }),
    )
    .await;
    assert_refused(&app, &client, no_agent, "guest agent").await;

    let (with_nic, nic_vm) =
        running_sandbox(&app, &client, "fork-nic", "firecracker", json!({})).await;
    sqlx::query(
        "INSERT INTO network_interfaces (vm_id, interface_type, device_id) \
         VALUES ($1, 'tap', 'net0')",
    )
    .bind(nic_vm)
    .execute(&app.pool)
    .await
    .unwrap();
    assert_refused(&app, &client, with_nic, "network interfaces").await;

    let (with_cloud_init, _) = running_sandbox(
        &app,
        &client,
        "fork-cloud-init",
        "firecracker",
        json!({ "cloud_init_user_data": "#cloud-config\n" }),
    )
    .await;
    assert_refused(&app, &client, with_cloud_init, "cloud-init").await;

    let pool = storage_pool(&app, "OVERLAYBD").await;
    let (with_data_disk, data_vm) =
        running_sandbox(&app, &client, "fork-data-disk", "firecracker", json!({})).await;
    add_disk(
        &app,
        data_vm,
        storage_object(&app, pool, "DISK").await,
        None,
    )
    .await;
    assert_refused(&app, &client, with_data_disk, "besides its OCI image's").await;

    let (with_persistent_layer, persistent_vm) =
        running_sandbox(&app, &client, "fork-persistent", "firecracker", json!({})).await;
    let image = storage_object(&app, pool, "OCI_IMAGE").await;
    let upper = storage_object(&app, pool, "OVERLAYBD_UPPER").await;
    add_disk(&app, persistent_vm, image, Some(upper)).await;
    assert_refused(
        &app,
        &client,
        with_persistent_layer,
        "persistent disk layer",
    )
    .await;

    // Nothing refused made a row.
    assert_eq!(count(&app, "vms").await, 8);
    assert_eq!(count(&app, "sandboxes").await, 8);
}

async fn storage_pool(app: &TestApp, pool_type: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO storage_pools (id, name, pool_type, status) \
         VALUES ($1, $2, $3::storage_pool_type, 'ACTIVE')",
    )
    .bind(id)
    .bind(format!("pool-{id}"))
    .bind(pool_type)
    .execute(&app.pool)
    .await
    .unwrap();
    id
}

async fn storage_object(app: &TestApp, pool: Uuid, object_type: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO storage_objects (id, name, storage_pool_id, object_type, size_bytes) \
         VALUES ($1, $2, $3, $4::storage_object_type, 1)",
    )
    .bind(id)
    .bind(format!("object-{id}"))
    .bind(pool)
    .bind(object_type)
    .execute(&app.pool)
    .await
    .unwrap();
    id
}

async fn add_disk(app: &TestApp, vm_id: Uuid, object: Uuid, upper: Option<Uuid>) {
    sqlx::query(
        "INSERT INTO vm_disks (vm_id, storage_object_id, logical_name, device_path, \
         upper_storage_object_id) VALUES ($1, $2, 'rootfs', '/dev/vda', $3)",
    )
    .bind(vm_id)
    .bind(object)
    .bind(upper)
    .execute(&app.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn a_fork_needs_room_on_its_source_host_and_a_failed_one_leaves_no_rows() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "fork-host").await;
    // Room for the source and one more VM like it.
    sqlx::query("UPDATE hosts SET total_memory_bytes = $1 WHERE id = $2")
        .bind(512 * MIB)
        .bind(host_id)
        .execute(&app.pool)
        .await
        .unwrap();
    // Booted from an OCI image: its image disk forks with it.
    let (sandbox_id, vm_id) =
        running_sandbox(&app, &client, "fork-room", "firecracker", json!({})).await;
    let pool = storage_pool(&app, "OVERLAYBD").await;
    add_disk(
        &app,
        vm_id,
        storage_object(&app, pool, "OCI_IMAGE").await,
        None,
    )
    .await;

    let (status, body) = fork(&app, &client, sandbox_id, json!({ "count": 2 })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains("no room for 2 more VMs"), "{body}");

    // One fits, and goes to the host, where no qarax-node listens.
    let (status, body) = fork(&app, &client, sandbox_id, json!({ "count": 1 })).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(count(&app, "vms").await, 1);
    assert_eq!(count(&app, "sandboxes").await, 1);
    assert_eq!(count(&app, "vm_disks").await, 1);
}
