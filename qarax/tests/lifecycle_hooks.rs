mod support;

use reqwest::StatusCode;
use serde_json::{Value, json};
use support::spawn_app;

async fn create_hook(client: &reqwest::Client, address: &str, body: Value) -> String {
    let res = client
        .post(format!("{address}/hooks"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    res.text().await.unwrap().trim_matches('"').to_string()
}

async fn update_hook(client: &reqwest::Client, address: &str, hook_id: &str, body: Value) -> Value {
    let res = client
        .patch(format!("{address}/hooks/{hook_id}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.json().await.unwrap()
}

#[tokio::test]
async fn hook_crud_round_trips() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let hook_id = create_hook(
        &client,
        &app.address,
        json!({"name": "crud-hook", "url": "https://example.com/hook", "events": ["running", "shutdown"]}),
    )
    .await;
    create_hook(
        &client,
        &app.address,
        json!({"name": "other-hook", "url": "https://example.com/other"}),
    )
    .await;
    let hook_url = format!("{}/hooks/{hook_id}", app.address);

    let res = client.get(&hook_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let hook: Value = res.json().await.unwrap();
    assert_eq!(hook["name"], "crud-hook");
    assert_eq!(hook["url"], "https://example.com/hook");
    assert_eq!(hook["scope"], "global");
    assert_eq!(hook["active"], true);
    assert_eq!(hook["events"], json!(["running", "shutdown"]));

    let res = client
        .get(format!("{}/hooks?name=crud-hook", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let hooks: Vec<Value> = res.json().await.unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["name"], "crud-hook");

    let updated = update_hook(
        &client,
        &app.address,
        &hook_id,
        json!({"url": "https://example.com/new", "active": false}),
    )
    .await;
    assert_eq!(updated["url"], "https://example.com/new");
    assert_eq!(updated["active"], false);
    assert_eq!(updated["name"], "crud-hook");

    let res = client.delete(&hook_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = client.get(&hook_url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hook_names_are_unique() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    create_hook(
        &client,
        &app.address,
        json!({"name": "dup-hook", "url": "https://example.com/hook"}),
    )
    .await;

    let res = client
        .post(format!("{}/hooks", app.address))
        .json(&json!({"name": "dup-hook", "url": "https://example.com/other"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn hook_update_can_clear_nullable_fields() {
    let app = spawn_app().await;
    let client = reqwest::Client::new();
    let hook_id = create_hook(
        &client,
        &app.address,
        json!({"name": "clear-hook", "url": "https://example.com/hook", "scope": "tag", "scope_value": "blue"}),
    )
    .await;
    update_hook(&client, &app.address, &hook_id, json!({"secret": "top-secret"})).await;

    // An explicit null clears the field; leaving a field out keeps it.
    let updated = update_hook(
        &client,
        &app.address,
        &hook_id,
        json!({"scope": "global", "scope_value": null, "secret": null}),
    )
    .await;
    assert_eq!(updated["scope"], "global");
    assert!(updated["scope_value"].is_null());
    assert!(updated["secret"].is_null());
    assert_eq!(updated["url"], "https://example.com/hook");
}
