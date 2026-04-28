use tokio::net::TcpListener;

use common::telemtry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use qarax::{
    configuration::{DatabaseSettings, default_control_plane_architecture, get_configuration},
    startup::run,
};
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::runtime::Runtime;
use uuid::Uuid;

struct TestApp {
    db_name: String,
    address: String,
    pool: PgPool,
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
    let mut configuration = get_configuration().expect("Failed to read configuration");
    configuration.database.name = Uuid::new_v4().to_string();
    let connection_pool = configure_database(&configuration.database).await;

    let server = run(
        listener,
        connection_pool.clone(),
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

async fn create_template(client: &reqwest::Client, address: &str, name: &str) -> Uuid {
    let res = client
        .post(format!("{address}/vm-templates"))
        .json(&json!({
            "name": name,
            "hypervisor": "firecracker",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "boot_mode": "kernel",
            "config": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    Uuid::parse_str(&res.text().await.unwrap()).unwrap()
}

#[tokio::test]
async fn sandbox_pool_supports_crud() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let template_id = create_template(&client, &app.address, "pool-template").await;

    let res = client
        .put(format!(
            "{}/vm-templates/{}/sandbox-pool",
            app.address, template_id
        ))
        .json(&json!({ "min_ready": 2 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let configured: serde_json::Value = res.json().await.unwrap();
    assert_eq!(configured["vm_template_id"], template_id.to_string());
    assert_eq!(configured["min_ready"], 2);
    assert_eq!(configured["current_ready"], 0);

    let res = client
        .get(format!(
            "{}/vm-templates/{}/sandbox-pool",
            app.address, template_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = client
        .get(format!("{}/sandbox-pools", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let pools: serde_json::Value = res.json().await.unwrap();
    assert_eq!(pools.as_array().unwrap().len(), 1);

    let res = client
        .delete(format!(
            "{}/vm-templates/{}/sandbox-pool",
            app.address, template_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn sandbox_create_claims_a_ready_pool_member() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let template_id = create_template(&client, &app.address, "claim-template").await;

    let res = client
        .put(format!(
            "{}/vm-templates/{}/sandbox-pool",
            app.address, template_id
        ))
        .json(&json!({ "min_ready": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let configured: serde_json::Value = res.json().await.unwrap();
    let pool_id = Uuid::parse_str(configured["id"].as_str().unwrap()).unwrap();

    let vm_id = Uuid::new_v4();
    let internal_name = format!("sandbox-pool-{}", &vm_id.to_string()[..8]);
    sqlx::query(
        r#"
INSERT INTO vms (
    id, name, tags, status, host_id, hypervisor, config,
    boot_vcpus, max_vcpus, memory_size, boot_mode
)
VALUES (
    $1, $2, ARRAY[]::TEXT[], 'RUNNING', NULL, 'FIRECRACKER', $3,
    1, 1, 268435456, 'KERNEL'
)
        "#,
    )
    .bind(vm_id)
    .bind(&internal_name)
    .bind(serde_json::json!({"sandbox_exec": true, "sandbox_prewarmed": true}))
    .execute(&app.pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
INSERT INTO sandbox_pool_members (id, sandbox_pool_id, vm_id, status)
VALUES ($1, $2, $3, 'READY')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(pool_id)
    .bind(vm_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let res = client
        .post(format!("{}/sandboxes", app.address))
        .json(&json!({
            "name": "claimed-sandbox",
            "vm_template_id": template_id,
            "idle_timeout_secs": 600
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let created: serde_json::Value = res.json().await.unwrap();
    let sandbox_id = created["id"].as_str().unwrap();
    let job_id = created["job_id"].as_str().unwrap();
    assert_eq!(created["vm_id"], vm_id.to_string());

    let res = client
        .get(format!("{}/sandboxes/{}", app.address, sandbox_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let sandbox: serde_json::Value = res.json().await.unwrap();
    assert_eq!(sandbox["status"], "ready");
    assert_eq!(sandbox["name"], "claimed-sandbox");

    let res = client
        .get(format!("{}/jobs/{}", app.address, job_id))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let job: serde_json::Value = res.json().await.unwrap();
    assert_eq!(job["status"], "completed");
    assert_eq!(job["job_type"], "sandbox_claim");

    let vm_name: String = sqlx::query_scalar("SELECT name FROM vms WHERE id = $1")
        .bind(vm_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(vm_name, "claimed-sandbox");

    let remaining_members: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM sandbox_pool_members WHERE sandbox_pool_id = $1",
    )
    .bind(pool_id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(remaining_members, 0);
}
