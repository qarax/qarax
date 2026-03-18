use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{DatabaseSettings, get_configuration},
    model::vm_disks,
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
    pub _pool: PgPool,
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

async fn configure_database(config: &DatabaseSettings) -> PgPool {
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
    let address = format!("http://127.0.0.1:{port}");
    let mut configuration =
        qarax::configuration::get_configuration().expect("Failed to read config");
    configuration.database.name = Uuid::new_v4().to_string();
    let connection_pool = configure_database(&configuration.database).await;

    let server = run(
        listener,
        connection_pool.clone(),
        configuration.vm_defaults.clone(),
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
        _pool: connection_pool,
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

async fn ensure_host_up(client: &reqwest::Client, address: &str) {
    let res = client
        .post(format!("{address}/hosts"))
        .json(&json!({
            "name": "test-host",
            "address": "127.0.0.1",
            "port": 50051,
            "host_user": "root",
            "password": ""
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let host_id = res.text().await.unwrap();

    let res = client
        .patch(format!("{address}/hosts/{host_id}"))
        .json(&json!({"status": "up"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

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
            "memory_size": 536870912
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
    ensure_host_up(&client, &app.address).await;

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
    ensure_host_up(&client, &app.address).await;

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
    ensure_host_up(&client, &app.address).await;

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
    ensure_host_up(&client, &app.address).await;
    let host_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM hosts WHERE name = 'test-host'")
        .fetch_one(&app._pool)
        .await
        .unwrap();
    let root_disk_object_id = create_local_pool_and_disk(&client, &app.address).await;
    let pool_id =
        sqlx::query_scalar::<_, Uuid>("SELECT storage_pool_id FROM storage_objects WHERE id = $1")
            .bind(Uuid::parse_str(&root_disk_object_id).unwrap())
            .fetch_one(&app._pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO host_storage_pools (host_id, storage_pool_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(host_id)
    .bind(pool_id)
    .execute(&app._pool)
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

    let disks = vm_disks::list_by_vm(&app._pool, vm_id).await.unwrap();
    assert_eq!(disks.len(), 1);
    assert_eq!(disks[0].logical_name, "rootfs");
    assert_eq!(disks[0].device_path, "/dev/vda");
    assert_eq!(disks[0].boot_order, Some(0));
    assert_eq!(
        disks[0].storage_object_id.unwrap().to_string(),
        root_disk_object_id
    );
}
