use crate::support::{create_vm, fake_node::add_forking_node, set_vm_status, spawn_app};
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

/// A Firecracker template named `name`, with `extra` set in its create body.
async fn create_template(
    client: &reqwest::Client,
    address: &str,
    name: &str,
    extra: serde_json::Value,
) -> Uuid {
    let mut body = json!({
        "name": name,
        "hypervisor": "firecracker",
        "boot_vcpus": 1,
        "max_vcpus": 1,
        "memory_size": 268435456,
        "boot_mode": "kernel",
        "config": {}
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    let res = client
        .post(format!("{address}/vm-templates"))
        .json(&body)
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
    let template_id = create_template(&client, &app.address, "pool-template", json!({})).await;
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
    assert_eq!(configured["fork"], false);
    assert_eq!(configured["seed_vm_id"], serde_json::Value::Null);

    let res = client
        .put(&pool_url)
        .json(&json!({ "min_ready": 2, "fork": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let configured: serde_json::Value = res.json().await.unwrap();
    assert_eq!(configured["fork"], true);

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
    assert_eq!(pools["items"].as_array().unwrap().len(), 1);

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
async fn a_pool_takes_a_template_that_boots_an_oci_image() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let template_id = create_template(
        &client,
        &app.address,
        "oci-pool-template",
        json!({ "image_ref": "registry.example/agent:latest" }),
    )
    .await;
    let pool_url = format!("{}/vm-templates/{template_id}/sandbox-pool", app.address);

    for fork in [false, true] {
        let res = client
            .put(&pool_url)
            .json(&json!({ "min_ready": 1, "fork": fork }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "fork: {fork}");
        let configured: serde_json::Value = res.json().await.unwrap();
        assert_eq!(configured["fork"], fork);
    }
}

#[tokio::test]
async fn sandbox_create_claims_a_ready_pool_member() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let template_id = create_template(&client, &app.address, "claim-template", json!({})).await;

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

/// A running VM in the pool as a READY member, or its seed.
async fn add_ready_member(app: &crate::support::TestApp, pool_id: Uuid, seed: bool) -> Uuid {
    let vm_id = Uuid::new_v4();
    sqlx::query(
        r#"
INSERT INTO vms (
    id, name, tags, status, host_id, hypervisor, config,
    boot_vcpus, max_vcpus, memory_size, boot_mode
)
VALUES (
    $1, $2, ARRAY[]::TEXT[], 'RUNNING', NULL, 'FIRECRACKER', '{}',
    1, 1, 268435456, 'KERNEL'
)
        "#,
    )
    .bind(vm_id)
    .bind(format!("sandbox-pool-{}", &vm_id.to_string()[..8]))
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sandbox_pool_members (sandbox_pool_id, vm_id, status, seed) \
         VALUES ($1, $2, 'READY', $3)",
    )
    .bind(pool_id)
    .bind(vm_id)
    .bind(seed)
    .execute(&app.pool)
    .await
    .unwrap();
    vm_id
}

#[tokio::test]
async fn a_forking_pool_s_seed_is_never_claimed() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let template_id = create_template(&client, &app.address, "fork-pool-template", json!({})).await;
    let pool_url = format!("{}/vm-templates/{template_id}/sandbox-pool", app.address);
    let res = client
        .put(&pool_url)
        .json(&json!({ "min_ready": 1, "fork": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let configured: serde_json::Value = res.json().await.unwrap();
    let pool_id = Uuid::parse_str(configured["id"].as_str().unwrap()).unwrap();
    let seed_vm = add_ready_member(&app, pool_id, true).await;
    let member_vm = add_ready_member(&app, pool_id, false).await;

    // The pool counts its members, and shows its seed apart.
    let pool: serde_json::Value = client
        .get(&pool_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pool["current_ready"], 1);
    assert_eq!(pool["seed_vm_id"], seed_vm.to_string());
    assert_eq!(pool["seed_status"], "ready");

    let claim = |name: &'static str| {
        client
            .post(format!("{}/sandboxes", app.address))
            .json(&json!({ "name": name, "vm_template_id": template_id }))
            .send()
    };
    let res = claim("first").await.unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let created: serde_json::Value = res.json().await.unwrap();
    assert_eq!(created["vm_id"], member_vm.to_string());

    // With only the seed left, a sandbox forks it. This one can't fork (no
    // guest agent, no host), so the sandbox boots a VM of its own instead,
    // here on no host at all.
    let res = claim("second").await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = res.text().await.unwrap();
    assert!(body.contains("no hosts in UP state"), "{body}");
    let pool: serde_json::Value = client
        .get(&pool_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pool["current_ready"], 0);
    assert_eq!(pool["seed_vm_id"], seed_vm.to_string());
}

#[tokio::test]
async fn a_sandbox_forks_the_seed_of_a_pool_with_no_member_ready() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (_, node) = add_forking_node(&app, "fork-node").await;
    let template_id = create_template(&client, &app.address, "seed-only-template", json!({})).await;
    let seed_vm = create_vm(
        &client,
        &app.address,
        json!({
            "name": "seed",
            "hypervisor": "firecracker",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 268435456,
            "guest_agent": true,
            "config": {}
        }),
    )
    .await;
    set_vm_status(&app.pool, seed_vm, "RUNNING").await;
    // The pool with its seed at once: the pool manager boots a seed of its own
    // for a forking pool without one.
    let mut tx = app.pool.begin().await.unwrap();
    let pool_id: Uuid = sqlx::query_scalar(
        "INSERT INTO sandbox_pools (vm_template_id, min_ready, fork) VALUES ($1, 0, true) \
         RETURNING id",
    )
    .bind(template_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sandbox_pool_members (sandbox_pool_id, vm_id, status, seed) \
         VALUES ($1, $2, 'READY', true)",
    )
    .bind(pool_id)
    .bind(seed_vm)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let res = client
        .post(format!("{}/sandboxes", app.address))
        .json(&json!({
            "name": "forked",
            "vm_template_id": template_id,
            "idle_timeout_secs": 600
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let created: serde_json::Value = res.json().await.unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();
    assert_ne!(vm_id, seed_vm.to_string());

    // The node forked the seed into the sandbox's VM.
    let forks = node.forks();
    assert_eq!(forks.len(), 1);
    assert_eq!(forks[0].source_vm_id, seed_vm.to_string());
    let fork_ids: Vec<&str> = forks[0].forks.iter().map(|f| f.vm_id.as_str()).collect();
    assert_eq!(fork_ids, [vm_id.as_str()]);

    // Ready when the request returns, as the request asked for it.
    let sandbox: serde_json::Value = client
        .get(format!(
            "{}/sandboxes/{}",
            app.address,
            created["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(sandbox["status"], "ready");
    assert_eq!(sandbox["vm_status"], "running");
    assert_eq!(sandbox["name"], "forked");
    assert_eq!(sandbox["idle_timeout_secs"], 600);
    let job: serde_json::Value = client
        .get(format!(
            "{}/jobs/{}",
            app.address,
            created["job_id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(job["status"], "completed");
    assert_eq!(job["job_type"], "sandbox_claim");
    assert_eq!(job["result"]["seed_vm_id"], seed_vm.to_string());

    // The seed stays the pool's, still its only member.
    let pool: serde_json::Value = client
        .get(format!(
            "{}/vm-templates/{template_id}/sandbox-pool",
            app.address
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pool["seed_vm_id"], seed_vm.to_string());
    assert_eq!(pool["seed_status"], "ready");
    assert_eq!(pool["current_ready"], 0);
}
