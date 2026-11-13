mod support;

use reqwest::StatusCode;
use serde_json::json;
use support::spawn_app;
use uuid::Uuid;

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
    let template_url = format!("{}/vm-templates/{template_id}", app.address);
    let pool_url = format!("{template_url}/sandbox-pool");

    let res = client
        .put(&pool_url)
        .json(&json!({ "min_ready": 2 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let configured: serde_json::Value = res.json().await.unwrap();
    assert_eq!(configured["vm_template_id"], template_id.to_string());
    assert_eq!(configured["min_ready"], 2);
    assert_eq!(configured["current_ready"], 0);

    let res = client.get(&pool_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let fetched: serde_json::Value = res.json().await.unwrap();
    assert_eq!(fetched["vm_template_id"], template_id.to_string());
    assert_eq!(fetched["min_ready"], 2);

    let res = client
        .get(format!("{}/sandbox-pools", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let pools: serde_json::Value = res.json().await.unwrap();
    assert_eq!(pools.as_array().unwrap().len(), 1);

    // The template can't go while a pool still draws from it.
    let res = client.delete(&template_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(res.text().await.unwrap().contains("sandbox pool"));

    let res = client.delete(&pool_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = client.get(&pool_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = client.delete(&template_url).send().await.unwrap();
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
