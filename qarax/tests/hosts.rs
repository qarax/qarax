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
