use uuid::Uuid;

use crate::client::Client;

use super::models::{HookExecution, LifecycleHook, NewLifecycleHook, UpdateLifecycleHook};

pub async fn list(client: &Client, name: Option<&str>) -> anyhow::Result<Vec<LifecycleHook>> {
    let path = match name {
        Some(n) => format!("/hooks?name={n}"),
        None => "/hooks".to_string(),
    };
    client.get(&path).await
}

pub async fn get(client: &Client, id: Uuid) -> anyhow::Result<LifecycleHook> {
    client.get(&format!("/hooks/{id}")).await
}

pub async fn create(client: &Client, hook: &NewLifecycleHook) -> anyhow::Result<String> {
    client.post_text("/hooks", hook).await
}

pub async fn update(
    client: &Client,
    id: Uuid,
    req: &UpdateLifecycleHook,
) -> anyhow::Result<LifecycleHook> {
    client.patch(&format!("/hooks/{id}"), req).await
}

pub async fn delete(client: &Client, id: Uuid) -> anyhow::Result<()> {
    client.delete(&format!("/hooks/{id}")).await
}

pub async fn list_executions(client: &Client, hook_id: Uuid) -> anyhow::Result<Vec<HookExecution>> {
    client.get(&format!("/hooks/{hook_id}/executions")).await
}
