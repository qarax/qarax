mod support;

use std::io::Write as _;

use qarax::model::host_gpus::{self, GpuDiscovery};
use reqwest::StatusCode;
use serde_json::json;
use support::{add_host, spawn_app};
use uuid::Uuid;

#[tokio::test]
async fn test_add_host_stores_only_credential_reference_and_hides_it() {
    let app = spawn_app().await;
    // A variable no environment sets, so resolution fails.
    let reference = "env://QARAX_TEST_UNSET_HOST_CREDENTIAL";
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/hosts", &app.address))
        .json(&json!({
            "name": "referenced-host",
            "address": "127.0.0.1",
            "port": 22,
            "host_user": "root",
            "credential_ref": reference,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let host_id: Uuid = response.text().await.unwrap().parse().unwrap();

    let stored_reference: Option<String> =
        sqlx::query_scalar("SELECT credential_ref FROM hosts WHERE id = $1")
            .bind(host_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(stored_reference.as_deref(), Some(reference));

    let response = client
        .get(format!("{}/hosts", &app.address))
        .send()
        .await
        .unwrap();
    let body = response.text().await.unwrap();
    assert!(!body.contains("credential_ref"));
    assert!(!body.contains("QARAX_TEST_UNSET_HOST_CREDENTIAL"));

    let response = client
        .post(format!("{}/hosts/{host_id}/deploy", &app.address))
        .json(&json!({ "image": "example.invalid/qarax:test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!response.text().await.unwrap().contains(reference));
}

#[tokio::test]
async fn test_deploy_resolves_file_credential_without_exposing_it() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let mut secret = tempfile::NamedTempFile::new().unwrap();
    writeln!(secret, "not-a-real-password").unwrap();
    let reference = format!("file://{}", secret.path().display());

    let response = client
        .post(format!("{}/hosts", &app.address))
        .json(&json!({
            "name": "file-credential-host",
            "address": "127.0.0.1",
            "port": 1,
            "host_user": "root",
            "credential_ref": reference,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let host_id: Uuid = response.text().await.unwrap().parse().unwrap();

    let body = client
        .get(format!("{}/hosts", &app.address))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!body.contains("credential_ref"));
    assert!(!body.contains("password"));
    assert!(!body.contains(&reference));
    assert!(!body.contains("not-a-real-password"));

    // The reference resolves before the SSH connection, which runs in the
    // background against a closed port.
    let response = client
        .post(format!("{}/hosts/{host_id}/deploy", &app.address))
        .json(&json!({ "image": "example.invalid/qarax:test", "ssh_port": 1, "reboot": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

/// Operators set up, down or maintenance. The other statuses record what
/// qarax did to the host, and a host is out of reach while a deploy runs.
#[tokio::test]
async fn test_update_host_status_is_limited_to_operator_statuses() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_host(&client, &app.address, "operator-status-host").await;
    let set_status = |status: &str| {
        client
            .patch(format!("{}/hosts/{host_id}", app.address))
            .json(&json!({ "status": status }))
            .send()
    };

    for status in [
        "installing",
        "installation_failed",
        "initializing",
        "unknown",
    ] {
        let response = set_status(status).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{status}"
        );
    }
    assert_eq!(
        set_status("maintenance").await.unwrap().status(),
        StatusCode::OK
    );

    sqlx::query("UPDATE hosts SET status = 'INSTALLING' WHERE id = $1")
        .bind(host_id)
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        set_status("up").await.unwrap().status(),
        StatusCode::CONFLICT
    );
}

/// A deploy that fails leaves the host INSTALLATION_FAILED and says why,
/// without the SSH password it was given.
#[tokio::test]
async fn test_failed_deploy_records_why() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_host(&client, &app.address, "failed-deploy-host").await;
    let password = "not-a-real-password";

    // Port 1 is closed, so the SSH connection fails right away.
    let response = client
        .post(format!("{}/hosts/{host_id}/deploy", &app.address))
        .json(&json!({
            "image": "example.invalid/qarax:test",
            "ssh_port": 1,
            "ssh_password": password,
            "reboot": false,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let host = loop {
        let hosts: Vec<serde_json::Value> = client
            .get(format!("{}/hosts", &app.address))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let host = hosts
            .into_iter()
            .find(|h| h["id"] == host_id.to_string())
            .unwrap();
        if host["status"] != "installing" {
            break host;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "deploy did not finish"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };

    assert_eq!(host["status"], "installation_failed");
    let error = host["installation_error"].as_str().unwrap();
    assert!(!error.is_empty());
    assert!(!error.contains(password), "{error}");
}

#[tokio::test]
async fn test_add_host_rejects_legacy_password_field() {
    let app = spawn_app().await;
    let response = reqwest::Client::new()
        .post(format!("{}/hosts", &app.address))
        .json(&json!({
            "name": "ambiguous-credentials",
            "address": "127.0.0.1",
            "port": 22,
            "host_user": "root",
            "password": "legacy-secret",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_list_host_gpus_returns_discovered_gpus() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = add_host(&client, &app.address, "gpu-host").await;
    host_gpus::sync_gpus(
        &app.pool,
        host_id,
        &[GpuDiscovery {
            pci_address: "0000:01:00.0".into(),
            model: "Test GPU".into(),
            vendor: "nvidia".into(),
            vram_bytes: 1 << 30,
            iommu_group: 7,
            numa_node: 0,
        }],
    )
    .await
    .unwrap();

    let res = client
        .get(format!("{}/hosts/{host_id}/gpus", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let gpus: Vec<serde_json::Value> = res.json().await.unwrap();
    assert_eq!(gpus.len(), 1);
    assert_eq!(gpus[0]["pci_address"], "0000:01:00.0");
    assert_eq!(gpus[0]["iommu_group"], 7);
    assert!(gpus[0]["vm_id"].is_null());

    let res = client
        .get(format!("{}/hosts/{}/gpus", &app.address, Uuid::new_v4()))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_host_refuses_while_vms_are_placed_on_it() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let host_id = support::add_up_host(&client, &app.address, "busy-host").await;
    let vm_id = support::create_vm(
        &client,
        &app.address,
        json!({
            "name": "vm-on-busy-host",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 256 * 1024 * 1024,
            "config": {}
        }),
    )
    .await;

    let res = client
        .delete(format!("{}/hosts/{host_id}", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("vm-on-busy-host"),
        "{body}"
    );

    let res = client
        .delete(format!("{}/vms/{vm_id}", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = client
        .delete(format!("{}/hosts/{host_id}", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = client
        .delete(format!("{}/hosts/{host_id}", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_host_drops_it_from_pending_storage_cleanups() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let gone = add_host(&client, &app.address, "cleanup-gone-host").await;
    let stays = add_host(&client, &app.address, "cleanup-stays-host").await;
    let res = client
        .post(format!("{}/storage-pools", &app.address))
        .json(&json!({
            "name": "cleanup-pool",
            "pool_type": "local",
            "config": {"path": "/tmp/cleanup-pool"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let pool_id: Uuid = res.text().await.unwrap().parse().unwrap();
    let object_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO storage_cleanup (object_id, pool_id, upper_layer, size_bytes, host_ids)
         VALUES ($1, $2, false, 0, $3)",
    )
    .bind(object_id)
    .bind(pool_id)
    .bind(vec![gone, stays])
    .execute(&app.pool)
    .await
    .unwrap();

    let res = client
        .delete(format!("{}/hosts/{gone}", &app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let host_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT host_ids FROM storage_cleanup WHERE object_id = $1")
            .bind(object_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(host_ids, [stays]);
}
