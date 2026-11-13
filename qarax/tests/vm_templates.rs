mod support;

use qarax::model::vm_disks;
use reqwest::StatusCode;
use serde_json::json;
use support::{add_up_host, spawn_app};
use uuid::Uuid;

async fn create_local_pool_and_disk(client: &reqwest::Client, address: &str) -> String {
    let res = client
        .post(format!("{address}/storage-pools"))
        .json(&json!({
            "name": "images",
            "pool_type": "local",
            "config": {"path": "/var/lib/qarax/images"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let pool_id = res.text().await.unwrap();

    let res = client
        .post(format!("{address}/storage-objects"))
        .json(&json!({
            "name": "ubuntu-root",
            "storage_pool_id": pool_id,
            "object_type": "disk",
            "size_bytes": 1073741824_i64,
            "config": {"path": "/var/lib/qarax/images/ubuntu-root.img"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    res.text().await.unwrap()
}

#[tokio::test]
async fn instance_types_and_vm_templates_support_crud() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();

    let res = client
        .post(format!("{}/instance-types", app.address))
        .json(&json!({
            "name": "small",
            "description": "Small instance type",
            "boot_vcpus": 2,
            "max_vcpus": 4,
            "memory_size": 536870912,
            "accelerator_config": {"gpu_count": 1, "gpu_vendor": "nvidia"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let instance_type_id = res.text().await.unwrap();

    let res = client
        .get(format!(
            "{}/instance-types/{}",
            app.address, instance_type_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let instance_type: serde_json::Value = res.json().await.unwrap();
    assert_eq!(instance_type["name"], "small");
    assert_eq!(instance_type["boot_vcpus"], 2);
    assert_eq!(instance_type["accelerator_config"]["gpu_count"], 1);
    assert_eq!(instance_type["accelerator_config"]["gpu_vendor"], "nvidia");

    let res = client
        .post(format!("{}/vm-templates", app.address))
        .json(&json!({
            "name": "ubuntu-base",
            "description": "Base Ubuntu template",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "boot_mode": "firmware",
            "config": {"os": "ubuntu"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let vm_template_id = res.text().await.unwrap();

    let res = client
        .get(format!("{}/vm-templates/{}", app.address, vm_template_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vm_template: serde_json::Value = res.json().await.unwrap();
    assert_eq!(vm_template["name"], "ubuntu-base");
    assert_eq!(vm_template["boot_mode"], "firmware");
    assert_eq!(vm_template["config"]["os"], "ubuntu");

    let res = client
        .get(format!("{}/instance-types", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let instance_types: serde_json::Value = res.json().await.unwrap();
    assert_eq!(instance_types.as_array().unwrap().len(), 1);

    let res = client
        .get(format!("{}/vm-templates", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vm_templates: serde_json::Value = res.json().await.unwrap();
    assert_eq!(vm_templates.as_array().unwrap().len(), 1);

    let res = client
        .delete(format!("{}/vm-templates/{}", app.address, vm_template_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let res = client
        .delete(format!(
            "{}/instance-types/{}",
            app.address, instance_type_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn create_vm_can_resolve_fields_from_vm_template() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;

    let res = client
        .post(format!("{}/vm-templates", app.address))
        .json(&json!({
            "name": "template-only",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 2,
            "max_vcpus": 2,
            "memory_size": 268435456,
            "boot_mode": "firmware",
            "description": "templated vm",
            "config": {"source": "template"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let vm_template_id = res.text().await.unwrap();

    let res = client
        .post(format!("{}/vms", app.address))
        .json(&json!({
            "name": "vm-from-template",
            "vm_template_id": vm_template_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let vm_id: String = res.json().await.unwrap();

    let res = client
        .get(format!("{}/vms/{}", app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vm: serde_json::Value = res.json().await.unwrap();
    assert_eq!(vm["hypervisor"], "cloud_hv");
    assert_eq!(vm["boot_vcpus"], 2);
    assert_eq!(vm["max_vcpus"], 2);
    assert_eq!(vm["memory_size"], 268435456_i64);
    assert_eq!(vm["boot_mode"], "firmware");
    assert_eq!(vm["description"], "templated vm");
    assert_eq!(vm["config"]["source"], "template");
}

#[tokio::test]
async fn create_vm_applies_precedence_direct_then_instance_type_then_template() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;

    let res = client
        .post(format!("{}/instance-types", app.address))
        .json(&json!({
            "name": "gpu-ish",
            "boot_vcpus": 4,
            "max_vcpus": 8,
            "memory_size": 1073741824
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let instance_type_id = res.text().await.unwrap();

    let res = client
        .post(format!("{}/vm-templates", app.address))
        .json(&json!({
            "name": "ai-template",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "boot_mode": "firmware",
            "description": "from-template",
            "config": {"layer": "template", "override": "template"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let vm_template_id = res.text().await.unwrap();

    let res = client
        .post(format!("{}/vms", app.address))
        .json(&json!({
            "name": "vm-layered",
            "vm_template_id": vm_template_id,
            "instance_type_id": instance_type_id,
            "boot_vcpus": 16,
            "description": "from-request",
            "config": {"override": "request", "request": true}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let vm_id: String = res.json().await.unwrap();

    let res = client
        .get(format!("{}/vms/{}", app.address, vm_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let vm: serde_json::Value = res.json().await.unwrap();

    assert_eq!(vm["hypervisor"], "cloud_hv");
    assert_eq!(vm["boot_vcpus"], 16);
    assert_eq!(vm["max_vcpus"], 16);
    assert_eq!(vm["memory_size"], 1073741824_i64);
    assert_eq!(vm["boot_mode"], "firmware");
    assert_eq!(vm["description"], "from-request");
    assert_eq!(vm["config"]["layer"], "template");
    assert_eq!(vm["config"]["override"], "request");
    assert_eq!(vm["config"]["request"], true);
}

#[tokio::test]
async fn create_vm_template_from_existing_vm_copies_reusable_fields() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;

    let res = client
        .post(format!("{}/vms", app.address))
        .json(&json!({
            "name": "source-vm",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 2,
            "max_vcpus": 4,
            "memory_size": 536870912,
            "boot_mode": "firmware",
            "description": "source-description",
            "cloud_init_user_data": "#cloud-config\nhostname: source\n",
            "config": {"origin": "source-vm"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let source_vm_id: String = res.json().await.unwrap();

    let res = client
        .post(format!("{}/vms/{}/template", app.address, source_vm_id))
        .json(&json!({
            "name": "copied-template"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let vm_template_id = res.text().await.unwrap();

    let res = client
        .get(format!("{}/vm-templates/{}", app.address, vm_template_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let template: serde_json::Value = res.json().await.unwrap();
    assert_eq!(template["name"], "copied-template");
    assert_eq!(template["description"], "source-description");
    assert_eq!(template["hypervisor"], "cloud_hv");
    assert_eq!(template["boot_vcpus"], 2);
    assert_eq!(template["max_vcpus"], 4);
    assert_eq!(template["memory_size"], 536870912_i64);
    assert_eq!(template["boot_mode"], "firmware");
    assert_eq!(
        template["cloud_init_user_data"],
        "#cloud-config\nhostname: source\n"
    );
    assert_eq!(template["config"]["origin"], "source-vm");

    let res = client
        .post(format!("{}/vms", app.address))
        .json(&json!({
            "name": "derived-vm",
            "vm_template_id": vm_template_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn create_vm_from_template_attaches_root_disk_object() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    add_up_host(&client, &app.address, "test-host").await;
    let host_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM hosts WHERE name = 'test-host'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let root_disk_object_id = create_local_pool_and_disk(&client, &app.address).await;
    let pool_id =
        sqlx::query_scalar::<_, Uuid>("SELECT storage_pool_id FROM storage_objects WHERE id = $1")
            .bind(Uuid::parse_str(&root_disk_object_id).unwrap())
            .fetch_one(&app.pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO host_storage_pools (host_id, storage_pool_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(host_id)
    .bind(pool_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let res = client
        .post(format!("{}/vm-templates", app.address))
        .json(&json!({
            "name": "disk-backed-template",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 2,
            "max_vcpus": 2,
            "memory_size": 536870912,
            "root_disk_object_id": root_disk_object_id,
            "boot_mode": "firmware"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let vm_template_id = res.text().await.unwrap();

    let res = client
        .post(format!("{}/vms", app.address))
        .json(&json!({
            "name": "disk-backed-vm",
            "vm_template_id": vm_template_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let vm_id: String = res.json().await.unwrap();
    let vm_id = Uuid::parse_str(&vm_id).unwrap();

    let disks = vm_disks::list_by_vm(&app.pool, vm_id).await.unwrap();
    assert_eq!(disks.len(), 1);
    assert_eq!(disks[0].logical_name, "rootfs");
    assert_eq!(disks[0].device_path, "/dev/vda");
    assert_eq!(disks[0].boot_order, Some(0));
    assert_eq!(
        disks[0].storage_object_id.unwrap().to_string(),
        root_disk_object_id
    );
}
