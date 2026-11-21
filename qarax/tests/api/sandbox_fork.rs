//! Forking a sandbox, short of the node: what is refused before any node
//! call, and that a fork whose node call fails leaves no rows behind. The fork
//! itself needs a real node; it is in e2e/test_sandboxes.py.

use crate::support::{TestApp, add_up_host, create_vm, set_vm_status, spawn_app};
use reqwest::StatusCode;
use serde_json::{Value, json};
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

async fn set_total_memory(app: &TestApp, host_id: Uuid, bytes: i64) {
    sqlx::query("UPDATE hosts SET total_memory_bytes = $1 WHERE id = $2")
        .bind(bytes)
        .bind(host_id)
        .execute(&app.pool)
        .await
        .unwrap();
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
async fn a_sandbox_that_cant_fork_is_refused_before_the_node() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "fork-host").await;

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
    assert_refused(
        &app,
        &client,
        with_nic,
        "outside a managed bridge or isolated network",
    )
    .await;

    let (cloud_init_network, cloud_init_vm) = running_sandbox(
        &app,
        &client,
        "fork-cloud-init-network",
        "firecracker",
        json!({
            "cloud_init_user_data": "#cloud-config\n",
            "cloud_init_network_config": "version: 2\n",
        }),
    )
    .await;
    add_nic(&app, host_id, cloud_init_vm).await;
    assert_refused(
        &app,
        &client,
        cloud_init_network,
        "network config comes from cloud-init",
    )
    .await;

    let pool = storage_pool(&app, "OVERLAYBD").await;
    let (with_data_disk, data_vm) =
        running_sandbox(&app, &client, "fork-data-disk", "firecracker", json!({})).await;
    add_disk(
        &app,
        data_vm,
        "vdb",
        storage_object(&app, pool, "DISK").await,
        None,
    )
    .await;
    assert_refused(&app, &client, with_data_disk, "on a local or NFS pool").await;

    let (with_persistent_layer, persistent_vm) =
        running_sandbox(&app, &client, "fork-persistent", "firecracker", json!({})).await;
    let image = storage_object(&app, pool, "OCI_IMAGE").await;
    let upper = storage_object(&app, pool, "OVERLAYBD_UPPER").await;
    add_disk(&app, persistent_vm, "rootfs", image, Some(upper)).await;
    assert_refused(
        &app,
        &client,
        with_persistent_layer,
        "persistent disk layer",
    )
    .await;

    // Nothing refused made a row.
    assert_eq!(count(&app, "vms").await, 7);
    assert_eq!(count(&app, "sandboxes").await, 7);
}

/// An isolated network attached to `host_id`, and a NIC of `vm_id` on it with
/// one of its addresses, as the API makes them.
async fn add_nic(app: &TestApp, host_id: Uuid, vm_id: Uuid) {
    let network_id = Uuid::new_v4();
    for (sql, bind_vm) in [
        (
            "INSERT INTO networks (id, name, subnet, gateway, type) \
             VALUES ($1, 'fork-net', '10.9.0.0/24', '10.9.0.1', 'isolated')",
            false,
        ),
        (
            "INSERT INTO ip_allocations (network_id, ip_address, vm_id) \
             VALUES ($1, '10.9.0.2', $2)",
            true,
        ),
        (
            "INSERT INTO network_interfaces (network_id, vm_id, interface_type, device_id, \
             ip_address) VALUES ($1, $2, 'tap', 'net0', '10.9.0.2')",
            true,
        ),
    ] {
        let query = sqlx::query(sql).bind(network_id);
        let query = if bind_vm { query.bind(vm_id) } else { query };
        query.execute(&app.pool).await.unwrap();
    }
    sqlx::query(
        "INSERT INTO host_networks (host_id, network_id, bridge_name) VALUES ($1, $2, 'brfork')",
    )
    .bind(host_id)
    .bind(network_id)
    .execute(&app.pool)
    .await
    .unwrap();
}

async fn add_security_group(app: &TestApp, vm_id: Uuid) {
    let group = Uuid::new_v4();
    sqlx::query("INSERT INTO security_groups (id, name) VALUES ($1, 'fork-sg')")
        .bind(group)
        .execute(&app.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO vm_security_groups (vm_id, security_group_id) VALUES ($1, $2)")
        .bind(vm_id)
        .bind(group)
        .execute(&app.pool)
        .await
        .unwrap();
}

async fn storage_pool(app: &TestApp, pool_type: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO storage_pools (id, name, pool_type, status, config) \
         VALUES ($1, $2, $3::storage_pool_type, 'ACTIVE', $4)",
    )
    .bind(id)
    .bind(format!("pool-{id}"))
    .bind(pool_type)
    .bind(json!({ "path": format!("/var/lib/qarax/pool-{id}") }))
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

async fn add_disk(app: &TestApp, vm_id: Uuid, name: &str, object: Uuid, upper: Option<Uuid>) {
    sqlx::query(
        "INSERT INTO vm_disks (vm_id, storage_object_id, logical_name, device_path, \
         upper_storage_object_id) VALUES ($1, $2, $3, '/dev/' || $3, $4)",
    )
    .bind(vm_id)
    .bind(object)
    .bind(name)
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
    // Room for the source and one fork, which shares the source's memory and
    // is charged a quarter of it until its node measures what it holds of its
    // own (scheduling.fork_memory_headroom_ratio).
    set_total_memory(&app, host_id, (256 + 100) * MIB).await;
    // Booted from an OCI image, with a data disk, a NIC on a network and a
    // security group: they all fork with it.
    let (sandbox_id, vm_id) =
        running_sandbox(&app, &client, "fork-room", "firecracker", json!({})).await;
    add_nic(&app, host_id, vm_id).await;
    add_security_group(&app, vm_id).await;
    let pool = storage_pool(&app, "OVERLAYBD").await;
    add_disk(
        &app,
        vm_id,
        "rootfs",
        storage_object(&app, pool, "OCI_IMAGE").await,
        None,
    )
    .await;
    let local = storage_pool(&app, "LOCAL").await;
    add_disk(
        &app,
        vm_id,
        "vdb",
        storage_object(&app, local, "DISK").await,
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
    assert_eq!(count(&app, "vm_disks").await, 2);
    // The fork's own image object and disk copy went with its row, the copy's
    // file to the cleanup queue.
    assert_eq!(count(&app, "storage_objects").await, 2);
    assert_eq!(count(&app, "storage_cleanup").await, 1);
    assert_eq!(count(&app, "network_interfaces").await, 1);
    assert_eq!(count(&app, "ip_allocations").await, 1);
    assert_eq!(count(&app, "vm_security_groups").await, 1);
}

#[tokio::test]
async fn a_fork_with_a_copy_of_its_source_s_memory_needs_room_for_all_of_it() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "fork-host").await;
    set_total_memory(&app, host_id, (256 + 100) * MIB).await;
    // Cloud Hypervisor restores a VM with shared memory with a copy of it.
    let (sandbox_id, _) = running_sandbox(
        &app,
        &client,
        "fork-shared",
        "cloud_hv",
        json!({ "memory_shared": true }),
    )
    .await;

    let (status, body) = fork(&app, &client, sandbox_id, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains("no room for 1 more VMs"), "{body}");
}
