use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{DatabaseSettings, default_control_plane_architecture, get_configuration},
    model::networks as network_model,
    startup::run,
};
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::runtime::Runtime;
use uuid::Uuid;

struct TestApp {
    pub db_name: String,
    pub address: String,
    pub pool: PgPool,
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

pub async fn configure_database(config: &DatabaseSettings) -> PgPool {
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
    let mut configuration =
        qarax::configuration::get_configuration().expect("Failed to read configuration.");
    configuration.database.name = Uuid::new_v4().to_string();
    let connection_pool = configure_database(&configuration.database).await;

    let server = run(
        listener,
        connection_pool.clone(),
        configuration.database.clone(),
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

/// Create a host and set it to UP so the scheduler can assign VMs.
async fn ensure_host_up(client: &reqwest::Client, address: &str) -> String {
    ensure_host_up_with_name(client, address, "test-host", 50051).await
}

async fn ensure_host_up_with_name(
    client: &reqwest::Client,
    address: &str,
    name: &str,
    port: u16,
) -> String {
    let res = client
        .post(format!("{}/hosts", address))
        .json(&json!({
            "name": name,
            "address": "127.0.0.1",
            "port": port,
            "host_user": "root",
            "password": ""
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let host_id = res.text().await.unwrap();

    client
        .patch(format!("{}/hosts/{}", address, host_id))
        .json(&json!({"status": "up"}))
        .send()
        .await
        .unwrap();

    host_id
}

async fn set_host_architecture(pool: &PgPool, host_id: &str, architecture: &str) {
    sqlx::query("UPDATE hosts SET architecture = $1 WHERE id = $2")
        .bind(architecture)
        .bind(Uuid::parse_str(host_id).unwrap())
        .execute(pool)
        .await
        .unwrap();
}

/// Create a VM via the API, returns the VM UUID string.
async fn create_vm(client: &reqwest::Client, address: &str, body: serde_json::Value) -> String {
    let res = client
        .post(format!("{}/vms", address))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED, "VM creation failed");
    res.json().await.unwrap()
}

#[tokio::test]
async fn test_create_vm_default_boot_mode() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "test-vm-kernel",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    let res = client
        .get(format!("{}/vms/{}", &app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vm: serde_json::Value = res.json().await.unwrap();
    assert_eq!(vm["boot_mode"], "kernel");
}

#[tokio::test]
async fn test_create_vm_firmware_boot_mode() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "test-vm-firmware",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "boot_mode": "firmware",
            "config": {}
        }),
    )
    .await;

    let res = client
        .get(format!("{}/vms/{}", &app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vm: serde_json::Value = res.json().await.unwrap();
    assert_eq!(vm["boot_mode"], "firmware");
}

#[tokio::test]
async fn test_create_vm_explicit_kernel_boot_mode() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "test-vm-explicit-kernel",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "boot_mode": "kernel",
            "config": {}
        }),
    )
    .await;

    let res = client
        .get(format!("{}/vms/{}", &app.address, vm_id))
        .send()
        .await
        .unwrap();
    let vm: serde_json::Value = res.json().await.unwrap();
    assert_eq!(vm["boot_mode"], "kernel");
}

#[tokio::test]
async fn test_list_vms_includes_boot_mode() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;

    create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-kernel",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-firmware",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "boot_mode": "firmware",
            "config": {}
        }),
    )
    .await;

    let res = client
        .get(format!("{}/vms", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vms: Vec<serde_json::Value> = res.json().await.unwrap();
    assert_eq!(vms.len(), 2);

    let kernel_vm = vms.iter().find(|v| v["name"] == "vm-kernel").unwrap();
    assert_eq!(kernel_vm["boot_mode"], "kernel");
    let firmware_vm = vms.iter().find(|v| v["name"] == "vm-firmware").unwrap();
    assert_eq!(firmware_vm["boot_mode"], "firmware");
}

#[tokio::test]
async fn test_create_vm_with_tags_round_trips() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "test-vm-tags",
            "tags": ["prod", "web"],
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    let res = client
        .get(format!("{}/vms/{}", &app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vm: serde_json::Value = res.json().await.unwrap();
    assert_eq!(vm["tags"], json!(["prod", "web"]));
}

#[tokio::test]
async fn test_list_vms_includes_tags() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;

    create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-tagged",
            "tags": ["batch", "blue"],
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-untagged",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    let res = client
        .get(format!("{}/vms", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vms: Vec<serde_json::Value> = res.json().await.unwrap();

    let tagged_vm = vms.iter().find(|v| v["name"] == "vm-tagged").unwrap();
    assert_eq!(tagged_vm["tags"], json!(["batch", "blue"]));

    let untagged_vm = vms.iter().find(|v| v["name"] == "vm-untagged").unwrap();
    assert_eq!(untagged_vm["tags"], json!([]));
}

/// Helper: create a local storage pool and storage object, and attach pool to host.
async fn create_test_storage_object(
    client: &reqwest::Client,
    address: &str,
    db_pool: &PgPool,
    host_id: &str,
    pool_name: &str,
    obj_name: &str,
) -> (String, String) {
    // Create storage pool (returns plain text)
    let res = client
        .post(format!("{}/storage-pools", address))
        .json(&json!({
            "name": pool_name,
            "pool_type": "local",
            "config": {"path": format!("/tmp/{}", pool_name)}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let pool_id = res.text().await.unwrap();

    // Attach pool to host directly in DB (bypasses gRPC to node)
    let host_uuid: Uuid = host_id.parse().unwrap();
    let pool_uuid: Uuid = pool_id.parse().unwrap();
    sqlx::query("INSERT INTO host_storage_pools (host_id, storage_pool_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(host_uuid)
        .bind(pool_uuid)
        .execute(db_pool)
        .await
        .unwrap();

    // Create storage object (returns plain text)
    let res = client
        .post(format!("{}/storage-objects", address))
        .json(&json!({
            "name": obj_name,
            "storage_pool_id": pool_id,
            "object_type": "disk",
            "size_bytes": 1073741824,
            "config": {"path": format!("/tmp/{}/{}", pool_name, obj_name)}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let object_id = res.text().await.unwrap();

    (pool_id, object_id)
}

async fn create_network(
    client: &reqwest::Client,
    address: &str,
    name: &str,
    subnet: &str,
    gateway: &str,
) -> String {
    let res = client
        .post(format!("{}/networks", address))
        .json(&json!({
            "name": name,
            "subnet": subnet,
            "gateway": gateway
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    res.text().await.unwrap()
}

async fn attach_network_to_host(pool: &PgPool, network_id: &str, host_id: &str, bridge_name: &str) {
    network_model::attach_host(
        pool,
        Uuid::parse_str(network_id).unwrap(),
        Uuid::parse_str(host_id).unwrap(),
        bridge_name,
    )
    .await
    .unwrap();
}

async fn count_ip_allocations_for_vm(pool: &PgPool, vm_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM ip_allocations WHERE vm_id = $1")
        .bind(Uuid::parse_str(vm_id).unwrap())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn count_nics_for_vm(pool: &PgPool, vm_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM network_interfaces WHERE vm_id = $1")
        .bind(Uuid::parse_str(vm_id).unwrap())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn count_security_groups_for_vm(pool: &PgPool, vm_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM vm_security_groups WHERE vm_id = $1")
        .bind(Uuid::parse_str(vm_id).unwrap())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_vm_status(pool: &PgPool, vm_id: &str, status: &str) {
    sqlx::query("UPDATE vms SET status = $1::vm_status WHERE id = $2")
        .bind(status)
        .bind(Uuid::parse_str(vm_id).unwrap())
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_attach_disk_auto_logical_name() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-disk-auto",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;
    let (_pool_id, object_id) = create_test_storage_object(
        &client,
        &app.address,
        &app.pool,
        &host_id,
        "test-pool",
        "test-disk",
    )
    .await;

    // Attach disk without specifying logical_name
    let res = client
        .post(format!("{}/vms/{}/disks", &app.address, vm_id))
        .json(&json!({
            "storage_object_id": object_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let disk: serde_json::Value = res.json().await.unwrap();
    assert_eq!(disk["logical_name"], "disk0");
    assert_eq!(disk["device_path"], "/dev/disk0");
}

#[tokio::test]
async fn test_create_network_with_vpc_name() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();

    let res = client
        .post(format!("{}/networks", app.address))
        .json(&json!({
            "name": "vpc-aware-net",
            "subnet": "10.77.0.0/24",
            "gateway": "10.77.0.1",
            "vpc_name": "dev-vpc"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let network_id = res.text().await.unwrap();

    let res = client
        .get(format!("{}/networks/{}", app.address, network_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let network: serde_json::Value = res.json().await.unwrap();
    assert_eq!(network["vpc_name"], "dev-vpc");
}

#[tokio::test]
async fn test_vm_security_group_bindings() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    ensure_host_up(&client, &app.address).await;

    let res = client
        .post(format!("{}/security-groups", app.address))
        .json(&json!({
            "name": "vm-bindings-sg",
            "description": "test group"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let security_group_id = res.text().await.unwrap();

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-with-sg",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "security_group_ids": [security_group_id],
            "config": {}
        }),
    )
    .await;

    assert_eq!(count_security_groups_for_vm(&app.pool, &vm_id).await, 1);

    let res = client
        .get(format!("{}/vms/{}/security-groups", app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let groups: serde_json::Value = res.json().await.unwrap();
    assert_eq!(groups.as_array().unwrap().len(), 1);

    let res = client
        .delete(format!(
            "{}/vms/{}/security-groups/{}",
            app.address, vm_id, security_group_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(count_security_groups_for_vm(&app.pool, &vm_id).await, 0);
}

#[tokio::test]
async fn test_attach_disk_explicit_logical_name() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-disk-explicit",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;
    let (_pool_id, object_id) = create_test_storage_object(
        &client,
        &app.address,
        &app.pool,
        &host_id,
        "expl-pool",
        "expl-disk",
    )
    .await;

    // Attach disk with explicit logical_name
    let res = client
        .post(format!("{}/vms/{}/disks", &app.address, vm_id))
        .json(&json!({
            "storage_object_id": object_id,
            "logical_name": "rootfs"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let disk: serde_json::Value = res.json().await.unwrap();
    assert_eq!(disk["logical_name"], "rootfs");
    assert_eq!(disk["device_path"], "/dev/rootfs");
}

#[tokio::test]
async fn test_attach_multiple_disks_auto_names() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-multi-disk",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    // Create storage pool (plain text response)
    let res = client
        .post(format!("{}/storage-pools", &app.address))
        .json(&json!({
            "name": "multi-pool",
            "pool_type": "local",
            "config": {"path": "/tmp/multi-pool"}
        }))
        .send()
        .await
        .unwrap();
    let pool_id = res.text().await.unwrap();

    // Attach pool to host directly in DB
    let host_uuid: Uuid = host_id.parse().unwrap();
    let pool_uuid: Uuid = pool_id.parse().unwrap();
    sqlx::query("INSERT INTO host_storage_pools (host_id, storage_pool_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(host_uuid)
        .bind(pool_uuid)
        .execute(&app.pool)
        .await
        .unwrap();

    let mut object_ids = vec![];
    for i in 0..3 {
        let res = client
            .post(format!("{}/storage-objects", &app.address))
            .json(&json!({
                "name": format!("disk-image-{}", i),
                "storage_pool_id": pool_id,
                "object_type": "disk",
                "size_bytes": 1073741824,
                "config": {"path": format!("/tmp/multi-pool/disk-{}", i)}
            }))
            .send()
            .await
            .unwrap();
        let oid = res.text().await.unwrap();
        object_ids.push(oid);
    }

    // Attach all three without specifying logical_name
    let mut names = vec![];
    for oid in &object_ids {
        let res = client
            .post(format!("{}/vms/{}/disks", &app.address, vm_id))
            .json(&json!({ "storage_object_id": oid }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let disk: serde_json::Value = res.json().await.unwrap();
        names.push(disk["logical_name"].as_str().unwrap().to_string());
    }

    assert_eq!(names, vec!["disk0", "disk1", "disk2"]);
}

#[tokio::test]
async fn test_attach_disk_duplicate_logical_name_rejected() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up(&client, &app.address).await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-dup-disk",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;

    // Create storage pool (plain text response)
    let res = client
        .post(format!("{}/storage-pools", &app.address))
        .json(&json!({
            "name": "dup-pool",
            "pool_type": "local",
            "config": {"path": "/tmp/dup-pool"}
        }))
        .send()
        .await
        .unwrap();
    let pool_id = res.text().await.unwrap();

    // Attach pool to host directly in DB
    let host_uuid: Uuid = host_id.parse().unwrap();
    let pool_uuid: Uuid = pool_id.parse().unwrap();
    sqlx::query("INSERT INTO host_storage_pools (host_id, storage_pool_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(host_uuid)
        .bind(pool_uuid)
        .execute(&app.pool)
        .await
        .unwrap();

    let mut oids = vec![];
    for i in 0..2 {
        let res = client
            .post(format!("{}/storage-objects", &app.address))
            .json(&json!({
                "name": format!("dup-disk-{}", i),
                "storage_pool_id": pool_id,
                "object_type": "disk",
                "size_bytes": 1073741824,
                "config": {"path": format!("/tmp/dup-pool/dup-disk-{}", i)}
            }))
            .send()
            .await
            .unwrap();
        let oid = res.text().await.unwrap();
        oids.push(oid);
    }

    // First attach with explicit name
    let res = client
        .post(format!("{}/vms/{}/disks", &app.address, vm_id))
        .json(&json!({
            "storage_object_id": oids[0],
            "logical_name": "mydata"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Second attach with the same name — should fail
    let res = client
        .post(format!("{}/vms/{}/disks", &app.address, vm_id))
        .json(&json!({
            "storage_object_id": oids[1],
            "logical_name": "mydata"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_client_error() || res.status().is_server_error(),
        "Expected error for duplicate logical_name, got {}",
        res.status()
    );
}

#[tokio::test]
async fn test_attach_disk_rejects_storage_object_used_by_other_vm() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up(&client, &app.address).await;

    let vm_owner = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-disk-owner",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;
    let vm_other = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-disk-other",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;
    let (_pool_id, object_id) = create_test_storage_object(
        &client,
        &app.address,
        &app.pool,
        &host_id,
        "shared-disk-pool",
        "shared-disk",
    )
    .await;

    let res = client
        .post(format!("{}/vms/{}/disks", &app.address, vm_owner))
        .json(&json!({
            "storage_object_id": object_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let res = client
        .post(format!("{}/vms/{}/disks", &app.address, vm_other))
        .json(&json!({
            "storage_object_id": object_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("vm-disk-owner")
    );
}

#[tokio::test]
async fn test_create_vm_rejects_root_disk_reused_by_other_vm() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up(&client, &app.address).await;

    let (_pool_id, object_id) = create_test_storage_object(
        &client,
        &app.address,
        &app.pool,
        &host_id,
        "root-disk-pool",
        "root-disk",
    )
    .await;

    let vm_owner = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-root-owner",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "root_disk_object_id": object_id,
            "config": {}
        }),
    )
    .await;
    assert!(!vm_owner.is_empty());

    let res = client
        .post(format!("{}/vms", &app.address))
        .json(&json!({
            "name": "vm-root-other",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "root_disk_object_id": object_id,
            "config": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("vm-root-owner")
    );
}

#[tokio::test]
async fn test_start_with_conflicting_disk_keeps_vm_created() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up(&client, &app.address).await;

    let vm_owner = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-start-owner",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;
    let vm_other = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-start-other",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;
    let (_pool_id, object_id) = create_test_storage_object(
        &client,
        &app.address,
        &app.pool,
        &host_id,
        "start-conflict-pool",
        "start-conflict-disk",
    )
    .await;

    let res = client
        .post(format!("{}/vms/{}/disks", &app.address, vm_owner))
        .json(&json!({
            "storage_object_id": object_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    sqlx::query(
        r#"
        INSERT INTO vm_disks (
            id, vm_id, storage_object_id, logical_name, device_path, boot_order,
            read_only, direct, vhost_user, vhost_socket,
            num_queues, queue_size, rate_limiter, rate_limit_group,
            pci_segment, serial_number, config, upper_storage_object_id
        )
        VALUES (
            $1, $2, $3, 'rootfs', '/dev/vda', 0,
            false, false, false, NULL,
            1, 128, NULL, NULL,
            0, NULL, '{}'::jsonb, NULL
        )
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::parse_str(&vm_other).unwrap())
    .bind(Uuid::parse_str(&object_id).unwrap())
    .execute(&app.pool)
    .await
    .unwrap();

    let res = client
        .post(format!("{}/vms/{}/start", &app.address, vm_other))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let res = client
        .get(format!("{}/vms/{}", &app.address, vm_other))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["status"], "created");
}

#[tokio::test]
async fn test_remove_nic_releases_ip_allocation() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up(&client, &app.address).await;

    let network_id = create_network(
        &client,
        &app.address,
        "remove-nic-net",
        "10.91.0.0/24",
        "10.91.0.1",
    )
    .await;
    attach_network_to_host(&app.pool, &network_id, &host_id, "testbr91").await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-remove-nic",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "network_id": network_id,
            "config": {}
        }),
    )
    .await;

    assert_eq!(count_ip_allocations_for_vm(&app.pool, &vm_id).await, 1);
    assert_eq!(count_nics_for_vm(&app.pool, &vm_id).await, 1);

    let res = client
        .delete(format!("{}/vms/{}/nics/net0", &app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    assert_eq!(count_ip_allocations_for_vm(&app.pool, &vm_id).await, 0);
    assert_eq!(count_nics_for_vm(&app.pool, &vm_id).await, 0);
}

#[tokio::test]
async fn test_add_nic_hotplug_failure_cleans_up_allocation_and_record() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up_with_name(&client, &app.address, "hotplug-host", 59999).await;

    let network_id = create_network(
        &client,
        &app.address,
        "hotplug-fail-net",
        "10.92.0.0/24",
        "10.92.0.1",
    )
    .await;
    attach_network_to_host(&app.pool, &network_id, &host_id, "testbr92").await;

    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-hotplug-fail",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "config": {}
        }),
    )
    .await;
    set_vm_status(&app.pool, &vm_id, "RUNNING").await;

    let res = client
        .post(format!("{}/vms/{}/nics", &app.address, vm_id))
        .json(&json!({
            "id": "",
            "network_id": network_id
        }))
        .send()
        .await
        .unwrap();
    let status = res.status();
    let body = res.text().await.unwrap();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");

    assert_eq!(count_ip_allocations_for_vm(&app.pool, &vm_id).await, 0);
    assert_eq!(count_nics_for_vm(&app.pool, &vm_id).await, 0);
}

#[tokio::test]
async fn test_preflight_requires_non_empty_image_ref() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();

    let res = client
        .post(format!("{}/vms/preflight", &app.address))
        .json(&json!({
            "image_ref": ""
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["message"], "image_ref is required");
}

#[tokio::test]
async fn test_preflight_rejects_selected_host_architecture_mismatch() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = ensure_host_up_with_name(&client, &app.address, "arch-host", 50051).await;
    set_host_architecture(&app.pool, &host_id, "x86_64").await;

    let res = client
        .post(format!("{}/vms/preflight", &app.address))
        .json(&json!({
            "image_ref": "registry:5000/test/busybox:latest",
            "host_id": host_id,
            "architecture": "aarch64"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("selected host architecture x86_64 does not match VM architecture aarch64")
    );
}
