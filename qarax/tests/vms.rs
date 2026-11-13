mod support;

use qarax::model::{
    networks as network_model, storage_pools, vm_disks,
    vms::{self, VmStatus},
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{add_up_host, create_vm, set_vm_status, spawn_app};
use uuid::Uuid;

const MIB: i64 = 1024 * 1024;
const GIB: i64 = 1024 * MIB;

/// The smallest VM create body; tests add the fields they care about.
fn vm_body(name: &str) -> Value {
    json!({
        "name": name,
        "hypervisor": "cloud_hv",
        "boot_vcpus": 1,
        "max_vcpus": 1,
        "memory_size": 256 * MIB,
        "config": {}
    })
}

fn with(mut body: Value, extra: Value) -> Value {
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    body
}

async fn get_json(client: &reqwest::Client, url: String) -> Value {
    let res = client.get(url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.json().await.unwrap()
}

async fn list_vm_names(client: &reqwest::Client, url: String) -> Vec<String> {
    let vms = get_json(client, url).await;
    let mut names: Vec<String> = vms
        .as_array()
        .unwrap()
        .iter()
        .map(|vm| vm["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

/// Asserts a 422 whose message contains `expected`.
async fn assert_unprocessable(res: reqwest::Response, expected: &str) {
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = res.json().await.unwrap();
    let message = body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(expected),
        "expected {expected:?} in {message:?}"
    );
}

async fn set_host_architecture(pool: &PgPool, host_id: Uuid, architecture: &str) {
    sqlx::query("UPDATE hosts SET architecture = $1 WHERE id = $2")
        .bind(architecture)
        .bind(host_id)
        .execute(pool)
        .await
        .unwrap();
}

/// Creates a local storage pool and attaches it to `host_id` in the database,
/// which skips the node call the attach endpoint would make.
async fn create_local_pool(
    client: &reqwest::Client,
    address: &str,
    db_pool: &PgPool,
    host_id: Uuid,
    name: &str,
) -> Uuid {
    let res = client
        .post(format!("{address}/storage-pools"))
        .json(&json!({
            "name": name,
            "pool_type": "local",
            "config": {"path": format!("/tmp/{name}")}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let pool_id = res.text().await.unwrap().parse().unwrap();
    storage_pools::attach_host(db_pool, pool_id, host_id)
        .await
        .unwrap();
    pool_id
}

async fn create_disk_object(
    client: &reqwest::Client,
    address: &str,
    pool_id: Uuid,
    name: &str,
    size_bytes: i64,
) -> Uuid {
    let res = client
        .post(format!("{address}/storage-objects"))
        .json(&json!({
            "name": name,
            "storage_pool_id": pool_id,
            "object_type": "disk",
            "size_bytes": size_bytes,
            "config": {"path": format!("/tmp/{pool_id}/{name}")}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    res.text().await.unwrap().parse().unwrap()
}

async fn attach_disk(
    client: &reqwest::Client,
    address: &str,
    vm_id: Uuid,
    body: Value,
) -> reqwest::Response {
    client
        .post(format!("{address}/vms/{vm_id}/disks"))
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn create_network(
    client: &reqwest::Client,
    address: &str,
    name: &str,
    subnet: &str,
    gateway: &str,
) -> Uuid {
    let res = client
        .post(format!("{address}/networks"))
        .json(&json!({
            "name": name,
            "subnet": subnet,
            "gateway": gateway
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    res.text().await.unwrap().parse().unwrap()
}

async fn attach_network_to_host(pool: &PgPool, network_id: Uuid, host_id: Uuid, bridge: &str) {
    network_model::attach_host(pool, network_id, host_id, bridge)
        .await
        .unwrap();
}

async fn count_for_vm(pool: &PgPool, table: &str, vm_id: Uuid) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE vm_id = $1"))
        .bind(vm_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_boot_mode_defaults_to_kernel_and_round_trips() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;

    let cases = [
        ("vm-default", None, "kernel"),
        ("vm-kernel", Some("kernel"), "kernel"),
        ("vm-firmware", Some("firmware"), "firmware"),
    ];
    for (name, boot_mode, expected) in cases {
        let mut body = vm_body(name);
        if let Some(boot_mode) = boot_mode {
            body["boot_mode"] = json!(boot_mode);
        }
        let vm_id = create_vm(&client, &app.address, body).await;
        let vm = get_json(&client, format!("{}/vms/{vm_id}", app.address)).await;
        assert_eq!(vm["boot_mode"], expected, "{name}");
    }

    let vms = get_json(&client, format!("{}/vms", app.address)).await;
    let vms = vms.as_array().unwrap();
    assert_eq!(vms.len(), cases.len());
    for (name, _, expected) in cases {
        let vm = vms.iter().find(|vm| vm["name"] == name).unwrap();
        assert_eq!(vm["boot_mode"], expected, "{name}");
    }
}

#[tokio::test]
async fn test_tags_round_trip_through_get_and_list() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;

    let tagged = create_vm(
        &client,
        &app.address,
        with(vm_body("vm-tagged"), json!({"tags": ["batch", "blue"]})),
    )
    .await;
    create_vm(&client, &app.address, vm_body("vm-untagged")).await;

    let vm = get_json(&client, format!("{}/vms/{tagged}", app.address)).await;
    assert_eq!(vm["tags"], json!(["batch", "blue"]));

    let vms = get_json(&client, format!("{}/vms", app.address)).await;
    let vms = vms.as_array().unwrap();
    let tagged_vm = vms.iter().find(|v| v["name"] == "vm-tagged").unwrap();
    assert_eq!(tagged_vm["tags"], json!(["batch", "blue"]));
    let untagged_vm = vms.iter().find(|v| v["name"] == "vm-untagged").unwrap();
    assert_eq!(untagged_vm["tags"], json!([]));
}

#[tokio::test]
async fn test_list_vms_filters_by_all_tags() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;
    create_vm(
        &client,
        &app.address,
        with(
            vm_body("vm-a"),
            json!({"tags": ["env:test", "team:platform"]}),
        ),
    )
    .await;
    create_vm(
        &client,
        &app.address,
        with(vm_body("vm-b"), json!({"tags": ["env:test"]})),
    )
    .await;

    let url = |tags: &str| format!("{}/vms?tags={tags}", app.address);
    assert_eq!(
        list_vm_names(&client, url("env:test")).await,
        ["vm-a", "vm-b"]
    );
    assert_eq!(
        list_vm_names(&client, url("env:test,team:platform")).await,
        ["vm-a"]
    );
    assert!(list_vm_names(&client, url("env:prod")).await.is_empty());
}

#[tokio::test]
async fn test_cloud_init_fields_round_trip() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;
    let user_data = "#cloud-config\npackages: []\n";
    let meta_data = "instance-id: cloud-init-test\n";
    let network_config = "version: 2\nethernets: {}\n";

    let without_network = create_vm(
        &client,
        &app.address,
        with(
            vm_body("vm-cloud-init"),
            json!({"cloud_init_user_data": user_data, "cloud_init_meta_data": meta_data}),
        ),
    )
    .await;
    let vm = get_json(&client, format!("{}/vms/{without_network}", app.address)).await;
    assert_eq!(vm["cloud_init_user_data"], user_data);
    assert_eq!(vm["cloud_init_meta_data"], meta_data);
    assert!(vm["cloud_init_network_config"].is_null());

    let with_network = create_vm(
        &client,
        &app.address,
        with(
            vm_body("vm-cloud-init-net"),
            json!({"cloud_init_user_data": user_data, "cloud_init_network_config": network_config}),
        ),
    )
    .await;
    let vm = get_json(&client, format!("{}/vms/{with_network}", app.address)).await;
    assert_eq!(vm["cloud_init_user_data"], user_data);
    assert_eq!(vm["cloud_init_network_config"], network_config);
}

#[tokio::test]
async fn test_ha_enabled_round_trips() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;

    let ha_vm = create_vm(
        &client,
        &app.address,
        with(vm_body("vm-ha"), json!({"ha_enabled": true})),
    )
    .await;
    let plain_vm = create_vm(&client, &app.address, vm_body("vm-no-ha")).await;

    let vm = get_json(&client, format!("{}/vms/{ha_vm}", app.address)).await;
    assert_eq!(vm["ha_enabled"], true);
    let vm = get_json(&client, format!("{}/vms/{plain_vm}", app.address)).await;
    assert_eq!(vm["ha_enabled"], false);
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

    let network = get_json(&client, format!("{}/networks/{network_id}", app.address)).await;
    assert_eq!(network["vpc_name"], "dev-vpc");
}

#[tokio::test]
async fn test_vm_security_group_bindings() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;

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
        with(
            vm_body("vm-with-sg"),
            json!({"security_group_ids": [security_group_id]}),
        ),
    )
    .await;

    assert_eq!(
        count_for_vm(&app.pool, "vm_security_groups", vm_id).await,
        1
    );

    let groups = get_json(
        &client,
        format!("{}/vms/{vm_id}/security-groups", app.address),
    )
    .await;
    assert_eq!(groups.as_array().unwrap().len(), 1);

    let res = client
        .delete(format!(
            "{}/vms/{vm_id}/security-groups/{security_group_id}",
            app.address
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        count_for_vm(&app.pool, "vm_security_groups", vm_id).await,
        0
    );
}

#[tokio::test]
async fn test_attach_disk_explicit_logical_name() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let vm_id = create_vm(&client, &app.address, vm_body("vm-disk-explicit")).await;
    let pool_id = create_local_pool(&client, &app.address, &app.pool, host_id, "expl-pool").await;
    let object_id = create_disk_object(&client, &app.address, pool_id, "expl-disk", GIB).await;

    let res = attach_disk(
        &client,
        &app.address,
        vm_id,
        json!({"storage_object_id": object_id, "logical_name": "rootfs"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let disk: Value = res.json().await.unwrap();
    assert_eq!(disk["logical_name"], "rootfs");
    assert_eq!(disk["device_path"], "/dev/rootfs");
}

#[tokio::test]
async fn test_attach_multiple_disks_auto_names() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let vm_id = create_vm(&client, &app.address, vm_body("vm-multi-disk")).await;
    let pool_id = create_local_pool(&client, &app.address, &app.pool, host_id, "multi-pool").await;

    let mut attached = vec![];
    for i in 0..3 {
        let object_id =
            create_disk_object(&client, &app.address, pool_id, &format!("disk-{i}"), GIB).await;
        let res = attach_disk(
            &client,
            &app.address,
            vm_id,
            json!({ "storage_object_id": object_id }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let disk: Value = res.json().await.unwrap();
        attached.push((
            disk["logical_name"].as_str().unwrap().to_string(),
            disk["device_path"].as_str().unwrap().to_string(),
        ));
    }

    assert_eq!(
        attached,
        [
            ("disk0".to_string(), "/dev/disk0".to_string()),
            ("disk1".to_string(), "/dev/disk1".to_string()),
            ("disk2".to_string(), "/dev/disk2".to_string()),
        ]
    );
}

#[tokio::test]
async fn test_attach_disk_duplicate_logical_name_rejected() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let vm_id = create_vm(&client, &app.address, vm_body("vm-dup-disk")).await;
    let pool_id = create_local_pool(&client, &app.address, &app.pool, host_id, "dup-pool").await;
    let first = create_disk_object(&client, &app.address, pool_id, "dup-disk-0", GIB).await;
    let second = create_disk_object(&client, &app.address, pool_id, "dup-disk-1", GIB).await;

    let res = attach_disk(
        &client,
        &app.address,
        vm_id,
        json!({"storage_object_id": first, "logical_name": "mydata"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let res = attach_disk(
        &client,
        &app.address,
        vm_id,
        json!({"storage_object_id": second, "logical_name": "mydata"}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(count_for_vm(&app.pool, "vm_disks", vm_id).await, 1);
}

#[tokio::test]
async fn test_attach_disk_rejects_storage_object_used_by_other_vm() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let vm_owner = create_vm(&client, &app.address, vm_body("vm-disk-owner")).await;
    let vm_other = create_vm(&client, &app.address, vm_body("vm-disk-other")).await;
    let pool_id = create_local_pool(
        &client,
        &app.address,
        &app.pool,
        host_id,
        "shared-disk-pool",
    )
    .await;
    let object_id = create_disk_object(&client, &app.address, pool_id, "shared-disk", GIB).await;

    let res = attach_disk(
        &client,
        &app.address,
        vm_owner,
        json!({"storage_object_id": object_id}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    let res = attach_disk(
        &client,
        &app.address,
        vm_other,
        json!({"storage_object_id": object_id}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body: Value = res.json().await.unwrap();
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
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let pool_id =
        create_local_pool(&client, &app.address, &app.pool, host_id, "root-disk-pool").await;
    let object_id = create_disk_object(&client, &app.address, pool_id, "root-disk", GIB).await;

    create_vm(
        &client,
        &app.address,
        with(
            vm_body("vm-root-owner"),
            json!({"root_disk_object_id": object_id}),
        ),
    )
    .await;

    let res = client
        .post(format!("{}/vms", &app.address))
        .json(&with(
            vm_body("vm-root-other"),
            json!({"root_disk_object_id": object_id}),
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body: Value = res.json().await.unwrap();
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("vm-root-owner")
    );
}

#[tokio::test]
async fn test_block_disk_has_one_writer_even_with_concurrent_attachments() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let pool_id = storage_pools::create(
        &app.pool,
        storage_pools::NewStoragePool {
            name: "block-pool".into(),
            pool_type: storage_pools::StoragePoolType::Block,
            config: json!({"portal": "127.0.0.1:3260", "iqn": "iqn.2026-10.qarax:test"}),
            capacity_bytes: None,
        },
    )
    .await
    .unwrap();
    storage_pools::attach_host(&app.pool, pool_id, host_id)
        .await
        .unwrap();

    let res = client
        .post(format!("{}/storage-pools/{pool_id}/luns", app.address))
        .json(&json!({"name": "lun0", "lun": 0, "size_bytes": GIB}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let lun: Value = res.json().await.unwrap();
    let object_id: Uuid = lun["storage_object_id"].as_str().unwrap().parse().unwrap();

    let mut vm_ids = Vec::new();
    for name in ["block-writer-a", "block-writer-b"] {
        vm_ids.push(create_vm(&client, &app.address, vm_body(name)).await);
    }
    let attach = |vm_id: Uuid| {
        client
            .post(format!("{}/vms/{vm_id}/disks", app.address))
            .json(&json!({"storage_object_id": object_id}))
            .send()
    };
    let (a, b) = tokio::join!(attach(vm_ids[0]), attach(vm_ids[1]));
    let mut statuses = [a.unwrap().status().as_u16(), b.unwrap().status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [201, 409]);
    let disks = vm_disks::list_by_storage_object(&app.pool, object_id)
        .await
        .unwrap();
    assert_eq!(disks.len(), 1);
    assert!(!disks[0].read_only);

    let response = client
        .post(format!("{}/vms", app.address))
        .json(&with(
            vm_body("block-root-writer"),
            json!({"root_disk_object_id": object_id}),
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    vm_disks::delete(&app.pool, disks[0].id).await.unwrap();
    assert_eq!(
        attach(vm_ids[0]).await.unwrap().status(),
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn test_start_with_conflicting_disk_keeps_vm_created() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let vm_owner = create_vm(&client, &app.address, vm_body("vm-start-owner")).await;
    let vm_other = create_vm(&client, &app.address, vm_body("vm-start-other")).await;
    let pool_id = create_local_pool(
        &client,
        &app.address,
        &app.pool,
        host_id,
        "start-conflict-pool",
    )
    .await;
    let object_id =
        create_disk_object(&client, &app.address, pool_id, "start-conflict-disk", GIB).await;

    let res = attach_disk(
        &client,
        &app.address,
        vm_owner,
        json!({"storage_object_id": object_id}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);

    // A second writer the API would refuse, as rows from older versions can hold.
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
    .bind(vm_other)
    .bind(object_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let res = client
        .post(format!("{}/vms/{vm_other}/start", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let vm = get_json(&client, format!("{}/vms/{vm_other}", app.address)).await;
    assert_eq!(vm["status"], "created");
}

/// On a VM that was never started, disk and NIC changes only touch the
/// database; no node call is made.
#[tokio::test]
async fn test_created_vm_disk_and_nic_changes() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let vm_id = create_vm(&client, &app.address, vm_body("vm-created-changes")).await;
    let pool_id =
        create_local_pool(&client, &app.address, &app.pool, host_id, "changes-pool").await;
    let object_id = create_disk_object(&client, &app.address, pool_id, "changes-disk", GIB).await;
    let vm_url = format!("{}/vms/{vm_id}", app.address);

    let res = attach_disk(
        &client,
        &app.address,
        vm_id,
        json!({"storage_object_id": object_id}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let res = client
        .delete(format!("{vm_url}/disks/disk0"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(count_for_vm(&app.pool, "vm_disks", vm_id).await, 0);
    let res = client
        .delete(format!("{vm_url}/disks/nonexistent"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let add_nic = || {
        client
            .post(format!("{vm_url}/nics"))
            .json(&json!({"id": "net0"}))
            .send()
    };
    let res = add_nic().await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let nic: Value = res.json().await.unwrap();
    assert_eq!(nic["device_id"], "net0");
    assert_eq!(add_nic().await.unwrap().status(), StatusCode::CONFLICT);

    let res = client
        .delete(format!("{vm_url}/nics/net0"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        count_for_vm(&app.pool, "network_interfaces", vm_id).await,
        0
    );
    let res = client
        .delete(format!("{vm_url}/nics/net99"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_remove_nic_releases_ip_allocation() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let network_id = create_network(
        &client,
        &app.address,
        "remove-nic-net",
        "10.91.0.0/24",
        "10.91.0.1",
    )
    .await;
    attach_network_to_host(&app.pool, network_id, host_id, "testbr91").await;

    let vm_id = create_vm(
        &client,
        &app.address,
        with(vm_body("vm-remove-nic"), json!({"network_id": network_id})),
    )
    .await;

    assert_eq!(count_for_vm(&app.pool, "ip_allocations", vm_id).await, 1);
    assert_eq!(
        count_for_vm(&app.pool, "network_interfaces", vm_id).await,
        1
    );

    let res = client
        .delete(format!("{}/vms/{vm_id}/nics/net0", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    assert_eq!(count_for_vm(&app.pool, "ip_allocations", vm_id).await, 0);
    assert_eq!(
        count_for_vm(&app.pool, "network_interfaces", vm_id).await,
        0
    );
}

#[tokio::test]
async fn test_add_nic_hotplug_failure_cleans_up_allocation_and_record() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "hotplug-host").await;
    let network_id = create_network(
        &client,
        &app.address,
        "hotplug-fail-net",
        "10.92.0.0/24",
        "10.92.0.1",
    )
    .await;
    attach_network_to_host(&app.pool, network_id, host_id, "testbr92").await;

    let vm_id = create_vm(&client, &app.address, vm_body("vm-hotplug-fail")).await;
    set_vm_status(&app.pool, vm_id, "RUNNING").await;

    let res = client
        .post(format!("{}/vms/{vm_id}/nics", &app.address))
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

    assert_eq!(count_for_vm(&app.pool, "ip_allocations", vm_id).await, 0);
    assert_eq!(
        count_for_vm(&app.pool, "network_interfaces", vm_id).await,
        0
    );
}

#[tokio::test]
async fn test_resize_rejects_invalid_requests() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;
    let memory_size = 256 * MIB;
    let hotplug_size = 256 * MIB;
    let vm_id = create_vm(
        &client,
        &app.address,
        with(
            vm_body("vm-resize"),
            json!({"max_vcpus": 4, "memory_size": memory_size, "memory_hotplug_size": hotplug_size}),
        ),
    )
    .await;
    let resize = |body: Value| {
        client
            .put(format!("{}/vms/{vm_id}/resize", app.address))
            .json(&body)
            .send()
    };

    assert_unprocessable(
        resize(json!({"desired_vcpus": 2})).await.unwrap(),
        "VM must be running to resize CPU or memory",
    )
    .await;

    // Everything below is rejected before the node is called.
    set_vm_status(&app.pool, vm_id, "RUNNING").await;
    let cases = [
        (
            json!({}),
            "At least one of desired_vcpus or desired_ram must be provided".to_string(),
        ),
        (
            json!({"desired_vcpus": 5}),
            "desired_vcpus 5 is out of range [1, 4]".to_string(),
        ),
        (
            json!({"desired_ram": memory_size + hotplug_size + 128 * MIB}),
            format!(
                "desired_ram {} is out of range",
                memory_size + hotplug_size + 128 * MIB
            ),
        ),
        (
            json!({"desired_ram": memory_size + 64 * MIB}),
            "is not a multiple of".to_string(),
        ),
    ];
    for (body, expected) in cases {
        assert_unprocessable(resize(body).await.unwrap(), &expected).await;
    }
}

#[tokio::test]
async fn test_disk_resize_rejects_invalid_requests() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "test-host").await;
    let vm_id = create_vm(&client, &app.address, vm_body("vm-disk-resize")).await;
    let pool_id = create_local_pool(&client, &app.address, &app.pool, host_id, "resize-pool").await;
    let object_id =
        create_disk_object(&client, &app.address, pool_id, "resize-disk", 2 * GIB).await;
    let res = attach_disk(
        &client,
        &app.address,
        vm_id,
        json!({"storage_object_id": object_id}),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let resize = |disk: &str, new_size_bytes: i64| {
        client
            .put(format!("{}/vms/{vm_id}/disks/{disk}/resize", app.address))
            .json(&json!({"new_size_bytes": new_size_bytes}))
            .send()
    };

    let res = resize("nonexistent", 4 * GIB).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_unprocessable(
        resize("disk0", GIB).await.unwrap(),
        "must be greater than current size",
    )
    .await;
    assert_unprocessable(
        resize("disk0", 2 * GIB + 500 * 1024).await.unwrap(),
        "must be a multiple of 1 MiB",
    )
    .await;

    set_vm_status(&app.pool, vm_id, "RUNNING").await;
    assert_unprocessable(
        resize("disk0", 4 * GIB).await.unwrap(),
        "VM must be stopped (Created or Shutdown) to resize a disk",
    )
    .await;
}

#[tokio::test]
async fn test_migrate_rejects_invalid_requests() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "source-host").await;
    let target = add_up_host(&client, &app.address, "target-host").await;
    let migrate = |vm_id: Uuid, target_host_id: Uuid| {
        client
            .post(format!("{}/vms/{vm_id}/migrate", app.address))
            .json(&json!({"target_host_id": target_host_id}))
            .send()
    };

    let res = migrate(Uuid::new_v4(), target).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let vm_id = create_vm(&client, &app.address, vm_body("vm-migrate")).await;
    assert_unprocessable(
        migrate(vm_id, target).await.unwrap(),
        "VM must be running or paused to migrate",
    )
    .await;

    set_vm_status(&app.pool, vm_id, "RUNNING").await;
    let vm = get_json(&client, format!("{}/vms/{vm_id}", app.address)).await;
    let current_host: Uuid = vm["host_id"].as_str().unwrap().parse().unwrap();
    assert_unprocessable(
        migrate(vm_id, current_host).await.unwrap(),
        "Source and destination hosts are the same",
    )
    .await;
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
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["message"], "image_ref is required");
}

#[tokio::test]
async fn test_preflight_rejects_selected_host_architecture_mismatch() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_up_host(&client, &app.address, "arch-host").await;
    set_host_architecture(&app.pool, host_id, "x86_64").await;

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

    assert_unprocessable(
        res,
        "selected host architecture x86_64 does not match VM architecture aarch64",
    )
    .await;
}

/// The VM monitor writes statuses it derived from an earlier read. A status
/// another writer set since that read must survive the monitor's write.
#[tokio::test]
async fn test_update_status_from_keeps_newer_status() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;
    let vm_id = create_vm(&client, &app.address, vm_body("test-vm-status-cas")).await;

    // The monitor reads the VM while it is still created...
    let stale = vms::get(&app.pool, vm_id).await.unwrap();
    assert_eq!(stale.status, VmStatus::Created);

    // ...a handler pauses it before the monitor writes...
    vms::update_status(&app.pool, vm_id, VmStatus::Paused)
        .await
        .unwrap();

    // ...so the monitor's write from the stale read is skipped.
    assert!(
        !vms::update_status_from(&app.pool, &stale, VmStatus::Running)
            .await
            .unwrap()
    );
    assert_eq!(
        vms::get(&app.pool, vm_id).await.unwrap().status,
        VmStatus::Paused
    );

    // A write from a current read goes through.
    let current = vms::get(&app.pool, vm_id).await.unwrap();
    assert!(
        vms::update_status_from(&app.pool, &current, VmStatus::Running)
            .await
            .unwrap()
    );
    assert_eq!(
        vms::get(&app.pool, vm_id).await.unwrap().status,
        VmStatus::Running
    );
}
