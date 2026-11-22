use crate::support::{
    TestApp, add_up_host, fake_node::add_forking_node, set_total_memory, spawn_app,
};
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
    assert_eq!(configured["seed_hosts"], 1);
    assert_eq!(configured["seeds"], json!([]));

    let res = client
        .put(&pool_url)
        .json(&json!({ "min_ready": 2, "fork": true, "seed_hosts": 2 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let configured: serde_json::Value = res.json().await.unwrap();
    assert_eq!(configured["fork"], true);
    assert_eq!(configured["seed_hosts"], 2);

    // A seed takes a host, and only a pool that forks has any.
    for (body, error) in [
        (
            json!({ "min_ready": 2, "fork": true, "seed_hosts": 0 }),
            "at least 1",
        ),
        (
            json!({ "min_ready": 2, "seed_hosts": 2 }),
            "for a pool that forks",
        ),
    ] {
        let res = client.put(&pool_url).json(&body).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        let text = res.text().await.unwrap();
        assert!(text.contains(error), "{body}: {text}");
    }

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

const MIB: i64 = 1024 * 1024;

/// A running 256 MiB Firecracker VM on `host_id`, with the guest agent when
/// `guest_agent`.
async fn running_vm(app: &TestApp, host_id: Uuid, guest_agent: bool) -> Uuid {
    let vm_id = Uuid::new_v4();
    sqlx::query(
        r#"
INSERT INTO vms (
    id, name, tags, status, host_id, hypervisor, config,
    boot_vcpus, max_vcpus, memory_size, boot_mode
)
VALUES (
    $1, $2, ARRAY[]::TEXT[], 'RUNNING', $3, 'FIRECRACKER', $4,
    1, 1, $5, 'KERNEL'
)
        "#,
    )
    .bind(vm_id)
    .bind(format!("sandbox-pool-{}", &vm_id.to_string()[..8]))
    .bind(host_id)
    .bind(json!({ "guest_agent": guest_agent }))
    .bind(256 * MIB)
    .execute(&app.pool)
    .await
    .unwrap();
    vm_id
}

/// The template's forking pool with its seeds and members, all ready and
/// made at once: the pool manager boots seeds of its own for a forking pool
/// short of them. The seeds are oldest first as listed.
async fn forking_pool(
    app: &TestApp,
    template_id: Uuid,
    min_ready: i32,
    seed_hosts: i32,
    seeds: &[Uuid],
    members: &[Uuid],
) -> Uuid {
    let mut tx = app.pool.begin().await.unwrap();
    let pool_id: Uuid = sqlx::query_scalar(
        "INSERT INTO sandbox_pools (vm_template_id, min_ready, fork, seed_hosts) \
         VALUES ($1, $2, true, $3) RETURNING id",
    )
    .bind(template_id)
    .bind(min_ready)
    .bind(seed_hosts)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let vms = seeds
        .iter()
        .map(|vm_id| (vm_id, true))
        .chain(members.iter().map(|vm_id| (vm_id, false)));
    for (age, (vm_id, seed)) in vms.enumerate() {
        sqlx::query(
            "INSERT INTO sandbox_pool_members (sandbox_pool_id, vm_id, status, seed, created_at) \
             VALUES ($1, $2, 'READY', $3, NOW() - make_interval(mins => 60 - $4))",
        )
        .bind(pool_id)
        .bind(vm_id)
        .bind(seed)
        .bind(age as i32)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    pool_id
}

async fn get_json(client: &reqwest::Client, url: String) -> serde_json::Value {
    let res = client.get(&url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "{url}");
    res.json().await.unwrap()
}

async fn claim(
    client: &reqwest::Client,
    app: &TestApp,
    template_id: Uuid,
    name: &str,
) -> reqwest::Response {
    client
        .post(format!("{}/sandboxes", app.address))
        .json(&json!({ "name": name, "vm_template_id": template_id, "idle_timeout_secs": 600 }))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn a_forking_pool_s_seed_is_never_claimed() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    // Room for the pool's two VMs and no more.
    let host_id = add_up_host(&client, &app.address, "pool-host").await;
    set_total_memory(&app, host_id, 512 * MIB).await;
    let template_id = create_template(&client, &app.address, "fork-pool-template", json!({})).await;
    // Without the guest agent, the seed can't fork.
    let seed_vm = running_vm(&app, host_id, false).await;
    let member_vm = running_vm(&app, host_id, false).await;
    forking_pool(&app, template_id, 1, 1, &[seed_vm], &[member_vm]).await;
    let pool_url = format!("{}/vm-templates/{template_id}/sandbox-pool", app.address);

    // The pool counts its members, and shows its seed apart.
    let pool = get_json(&client, pool_url.clone()).await;
    assert_eq!(pool["current_ready"], 1);
    assert_eq!(
        pool["seeds"],
        json!([{ "vm_id": seed_vm, "host_id": host_id, "status": "ready" }])
    );

    let res = claim(&client, &app, template_id, "first").await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let created: serde_json::Value = res.json().await.unwrap();
    assert_eq!(created["vm_id"], member_vm.to_string());

    // With only the seed left, a sandbox forks it. This one can't, so the
    // sandbox boots a VM of its own instead, and its host has no room left.
    let res = claim(&client, &app, template_id, "second").await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = res.text().await.unwrap();
    assert!(body.contains("no hosts in UP state"), "{body}");
    let pool = get_json(&client, pool_url).await;
    assert_eq!(pool["current_ready"], 0);
    assert_eq!(pool["seeds"][0]["vm_id"], seed_vm.to_string());
}

#[tokio::test]
async fn a_sandbox_forks_the_seed_of_a_pool_with_no_member_ready() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (host_id, node) = add_forking_node(&app, "fork-node").await;
    let template_id = create_template(&client, &app.address, "seed-only-template", json!({})).await;
    let seed_vm = running_vm(&app, host_id, true).await;
    forking_pool(&app, template_id, 0, 1, &[seed_vm], &[]).await;

    let res = claim(&client, &app, template_id, "forked").await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let created: serde_json::Value = res.json().await.unwrap();
    let vm_id = created["vm_id"].as_str().unwrap().to_string();
    assert_ne!(vm_id, seed_vm.to_string());

    // The node forked the seed into the sandbox's VM, from the seed's
    // snapshot it keeps rather than from its current state.
    assert!(node.forks().is_empty());
    let forks = node.seed_forks();
    assert_eq!(forks.len(), 1);
    assert_eq!(forks[0].source_vm_id, seed_vm.to_string());
    let fork_ids: Vec<&str> = forks[0].forks.iter().map(|f| f.vm_id.as_str()).collect();
    assert_eq!(fork_ids, [vm_id.as_str()]);

    // Ready when the request returns, as the request asked for it.
    let sandbox = get_json(
        &client,
        format!(
            "{}/sandboxes/{}",
            app.address,
            created["id"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(sandbox["status"], "ready");
    assert_eq!(sandbox["vm_status"], "running");
    assert_eq!(sandbox["name"], "forked");
    assert_eq!(sandbox["idle_timeout_secs"], 600);
    let job = get_json(
        &client,
        format!(
            "{}/jobs/{}",
            app.address,
            created["job_id"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(job["status"], "completed");
    assert_eq!(job["job_type"], "sandbox_claim");
    assert_eq!(job["result"]["seed_vm_id"], seed_vm.to_string());

    // The seed stays the pool's, still its only member.
    let pool = get_json(
        &client,
        format!("{}/vm-templates/{template_id}/sandbox-pool", app.address),
    )
    .await;
    assert_eq!(
        pool["seeds"],
        json!([{ "vm_id": seed_vm, "host_id": host_id, "status": "ready" }])
    );
    assert_eq!(pool["current_ready"], 0);
}

#[tokio::test]
async fn a_pool_forks_the_seed_on_a_host_with_room() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let (roomy, node) = add_forking_node(&app, "roomy").await;
    // Room for its seed and no fork of it.
    let full = add_up_host(&client, &app.address, "full").await;
    set_total_memory(&app, full, 300 * MIB).await;
    let template_id = create_template(&client, &app.address, "two-seed-template", json!({})).await;
    let full_seed = running_vm(&app, full, true).await;
    let roomy_seed = running_vm(&app, roomy, true).await;
    // Neither host holds a member: the older seed, the full host's, is tried
    // first.
    forking_pool(&app, template_id, 1, 2, &[full_seed, roomy_seed], &[]).await;
    let pool_url = format!("{}/vm-templates/{template_id}/sandbox-pool", app.address);

    let res = claim(&client, &app, template_id, "forked").await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let created: serde_json::Value = res.json().await.unwrap();
    // The claim starts a refill: the pool keeps a member ready.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while get_json(&client, pool_url.clone()).await["current_ready"] != 1 {
        assert!(
            std::time::Instant::now() < deadline,
            "the pool didn't refill"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // Both the sandbox and the member are forks of the seed with room, on its
    // host, from the snapshot of it the node keeps.
    assert!(node.forks().is_empty());
    let forks = node.seed_forks();
    assert_eq!(forks.len(), 2, "{forks:?}");
    assert!(
        forks
            .iter()
            .all(|fork| fork.source_vm_id == roomy_seed.to_string()),
        "{forks:?}"
    );
    let fork_vms: Vec<Uuid> = forks
        .iter()
        .flat_map(|fork| &fork.forks)
        .map(|fork| Uuid::parse_str(&fork.vm_id).unwrap())
        .collect();
    assert!(fork_vms.contains(&Uuid::parse_str(created["vm_id"].as_str().unwrap()).unwrap()));
    let hosts: Vec<Uuid> = sqlx::query_scalar("SELECT host_id FROM vms WHERE id = ANY($1)")
        .bind(&fork_vms)
        .fetch_all(&app.pool)
        .await
        .unwrap();
    assert_eq!(hosts, [roomy, roomy]);
}
