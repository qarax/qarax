use crate::support::{add_up_host, create_vm, spawn_app_with_auth};
use qarax::configuration::AuthSettings;
use reqwest::{
    StatusCode,
    header::{
        AUTHORIZATION, CONNECTION, HeaderMap, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_PROTOCOL,
        SEC_WEBSOCKET_VERSION, UPGRADE,
    },
};
use serde_json::json;

#[tokio::test]
async fn test_auth_disabled_allows_unauthenticated_requests() {
    let app = spawn_app_with_auth(AuthSettings {
        enabled: false,
        tokens: vec![],
    })
    .await;

    let res = reqwest::get(format!("{}/hosts", app.address))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_enabled_rejects_missing_token() {
    let app = spawn_app_with_auth(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;

    let res = reqwest::get(format!("{}/hosts", app.address))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_enabled_rejects_wrong_token() {
    let app = spawn_app_with_auth(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;

    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/hosts", app.address))
        .bearer_auth("wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_enabled_accepts_valid_token() {
    let app = spawn_app_with_auth(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;

    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/hosts", app.address))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_auth_enabled_allows_public_paths_without_token() {
    let app = spawn_app_with_auth(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;

    // reqwest follows the swagger-ui redirect, so every path ends in a 200.
    // Every page of the web UI is its index.html, which asks for a token.
    for path in [
        "",
        "/swagger-ui",
        "/api-docs/openapi.json",
        "/ui",
        "/ui/vms/some-id",
    ] {
        let res = reqwest::get(format!("{}{path}", app.root)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{path}");
    }
}

/// A browser can't set the Authorization header on a WebSocket upgrade, so the
/// web UI's console offers its token as a subprotocol, and the browser drops
/// the connection unless the server selects the console's subprotocol.
#[tokio::test]
async fn test_console_upgrade_takes_the_token_as_a_subprotocol() {
    let app = spawn_app_with_auth(AuthSettings {
        enabled: true,
        tokens: vec!["test-token".to_string()],
    })
    .await;
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, "Bearer test-token".parse().unwrap());
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap();
    add_up_host(&client, &app.address, "console-host").await;
    let vm_id = create_vm(
        &client,
        &app.address,
        json!({
            "name": "console-vm",
            "hypervisor": "cloud_hv",
            "boot_vcpus": 1,
            "max_vcpus": 1,
            "memory_size": 256 * 1024 * 1024,
            "config": {}
        }),
    )
    .await;

    let upgrade = |token: &str| {
        reqwest::Client::new()
            .get(format!("{}/vms/{vm_id}/console/attach", app.address))
            .header(CONNECTION, "upgrade")
            .header(UPGRADE, "websocket")
            .header(SEC_WEBSOCKET_VERSION, "13")
            .header(SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ==")
            .header(
                SEC_WEBSOCKET_PROTOCOL,
                format!("qarax.console, qarax.bearer.{}", hex::encode(token)),
            )
            .send()
    };

    let res = upgrade("test-token").await.unwrap();
    assert_eq!(res.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert_eq!(res.headers()[SEC_WEBSOCKET_PROTOCOL], "qarax.console");

    let res = upgrade("wrong-token").await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
