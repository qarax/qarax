mod support;

use qarax::configuration::AuthSettings;
use reqwest::StatusCode;
use support::spawn_app_with_auth;

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
    for path in ["", "/swagger-ui", "/api-docs/openapi.json"] {
        let res = reqwest::get(format!("{}{path}", app.address))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{path}");
    }
}
